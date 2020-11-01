use futures::StreamExt;
use futures_util::future::TryFutureExt;
use hyper::{http::StatusCode, Body, Request, Response};
use itertools::Itertools;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
	auth::GithubUserAuthenticator, companion::*, config::BotConfig,
	constants::*, error::*, github::*, github_bot::GithubBot, gitlab_bot::*,
	matrix_bot::MatrixBot, performance, process, rebase::*, Result,
};

/// This data gets passed along with each webhook to the webhook handler.
pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub gitlab_bot: GitlabBot,

	pub bot_config: BotConfig,
	pub webhook_secret: String,
}

/// This stores information about a pull request while we wait for checks to complete.
#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	owner: String,
	repo_name: String,
	number: i64,
	html_url: String,
	requested_by: String,
}

/// Check the SHA1 signature on a webhook payload.
fn verify(
	secret: &[u8],
	msg: &[u8],
	signature: &[u8],
) -> Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	hmac::verify(&key, msg, signature)
}

/// Receive a webhook and state object, acquire lock on state object.
pub async fn webhook(
	req: Request<Body>,
	state: Arc<Mutex<AppState>>,
) -> Result<Response<Body>> {
	if req.uri().path() == "/webhook" {
		// lock here to prevent double merge requests being sent (which often happens when checks
		// complete because we receive redundant status hooks).
		let state = &*state.lock().await;
		let sig = req
			.headers()
			.get("x-hub-signature")
			.context(Message {
				msg: format!("Missing x-hub-signature"),
			})?
			.to_str()
			.ok()
			.context(Message {
				msg: format!("Error parsing x-hub-signature"),
			})?
			.to_string();
		log::info!("Lock acquired for {:?}", sig);
		if let Err(e) = webhook_inner(req, state).await {
			handle_error(e, state).await;
		}
		log::info!("Will release lock for {:?}", sig);
		Response::builder()
			.status(StatusCode::OK)
			.body(Body::from(""))
			.ok()
			.context(Message {
				msg: format!("Error building response"),
			})
	} else {
		Response::builder()
			.status(StatusCode::NOT_FOUND)
			.body(Body::from("Not found."))
			.ok()
			.context(Message {
				msg: format!("Error building response"),
			})
	}
}

/// Parse webhook body and verify.
pub async fn webhook_inner(
	mut req: Request<Body>,
	state: &AppState,
) -> Result<()> {
	let mut msg_bytes = vec![];
	while let Some(item) = req.body_mut().next().await {
		msg_bytes.extend_from_slice(&item.ok().context(Message {
			msg: format!("Error getting bytes from request body"),
		})?);
	}

	let sig = req
		.headers()
		.get("x-hub-signature")
		.context(Message {
			msg: format!("Missing x-hub-signature"),
		})?
		.to_str()
		.ok()
		.context(Message {
			msg: format!("Error parsing x-hub-signature"),
		})?
		.replace("sha1=", "");
	let sig_bytes = base16::decode(sig.as_bytes()).ok().context(Message {
		msg: format!("Error decoding x-hub-signature"),
	})?;

	verify(
		state.webhook_secret.trim().as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.ok()
	.context(Message {
		msg: format!("Validation signature does not match"),
	})?;

	let payload = serde_json::from_slice::<Payload>(&msg_bytes).ok().context(
		Message {
			msg: format!("Error parsing request body"),
		},
	)?;

	handle_payload(payload, state).await
}

/// Match different kinds of payload.
async fn handle_payload(payload: Payload, state: &AppState) -> Result<()> {
	match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			comment:
				Comment {
					body,
					user: User { login, .. },
					..
				},
			issue:
				Issue {
					number,
					html_url,
					repository_url: Some(repo_url),
					pull_request: Some(_), // indicates the issue is a pr
					..
				},
		} => {
			handle_comment(body, login, number, html_url, repo_url, state).await
		}
		Payload::CommitStatus {
			sha, state: status, ..
		} => handle_status(sha, status, state).await,
		Payload::CheckRun {
			check_run: CheckRun {
				status, head_sha, ..
			},
			..
		} => handle_check(status, head_sha, state).await,
		_event => Ok(()),
	}
}

/// If a check completes, query if all statuses and checks are complete.
async fn handle_check(
	status: String,
	commit_sha: String,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;

	if status == "completed".to_string() {
		checks_and_status(github_bot, &commit_sha, db).await?;
	}

	Ok(())
}

/// If we receive a status other than `Pending`, query if all statuses and checks are complete.
async fn handle_status(
	commit_sha: String,
	status: StatusState,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;

	if status != StatusState::Pending {
		checks_and_status(github_bot, &commit_sha, db).await?;
	}
	Ok(())
}

/// Check that no commit has been pushed since the merge request was received.  Query checks and
/// statuses and if they are green, attempt merge.
async fn checks_and_status(
	github_bot: &GithubBot,
	commit_sha: &str,
	db: &DB,
) -> Result<()> {
	if let Some(b) = db.get(commit_sha.trim().as_bytes()).context(Db)? {
		let m = bincode::deserialize(&b).context(Bincode)?;
		log::info!("Deserialized merge request: {:?}", m);
		let MergeRequest {
			owner,
			repo_name,
			number,
			html_url,
			requested_by: _,
		} = m;
		let pr = github_bot.pull_request(&owner, &repo_name, number).await?;

		// Head sha should not have changed since request was made.
		if commit_sha == pr.head.sha {
			log::info!(
				"Commit sha {} matches head of {}",
				commit_sha,
				html_url
			);

			// Delay after status hook to avoid false success
			tokio::time::delay_for(std::time::Duration::from_millis(1000))
				.await;

			let checks = github_bot
				.check_runs(&owner, &repo_name, &commit_sha)
				.await?;
			log::info!("{:?}", checks);
			if checks
				.check_runs
				.iter()
				.all(|r| r.conclusion == Some("success".to_string()))
			{
				log::info!("All checks success");
				let status =
					github_bot.status(&owner, &repo_name, &commit_sha).await?;
				log::info!("{:?}", status);
				match status {
					CombinedStatus {
						state: StatusState::Success,
						..
					} => {
						log::info!("{} is green; attempting merge.", html_url);

						// to reach here merge must be allowed
						merge(github_bot, &owner, &repo_name, &pr).await?;

						// clean db
						db.delete(pr.head.sha.trim().as_bytes())
							.context(Db)
							.map_err(|e| {
								e.map_issue(Some((
									owner.to_string(),
									repo_name.to_string(),
									pr.number,
								)))
							})?;

						// update companion if necessary
						update_companion(github_bot, &repo_name, &pr, db)
							.await?;
					}
					CombinedStatus {
						state: StatusState::Failure,
						..
					} => {
						log::info!("{} status failure.", html_url);
						Err(Error::ChecksFailed {
							commit_sha: commit_sha.to_string(),
						}
						.map_issue(Some((
							owner.to_string(),
							repo_name.to_string(),
							pr.number,
						))))?;
					}
					CombinedStatus {
						state: StatusState::Error,
						..
					} => {
						log::info!("{} status error.", html_url);
						Err(Error::ChecksFailed {
							commit_sha: commit_sha.to_string(),
						}
						.map_issue(Some((
							owner.to_string(),
							repo_name.to_string(),
							pr.number,
						))))?;
					}
					CombinedStatus {
						state: StatusState::Pending,
						..
					} => {
						log::info!("{} is pending.", html_url);
					}
				}
			} else if checks
				.check_runs
				.iter()
				.all(|r| r.status == "completed".to_string())
			{
				log::info!("{} checks were unsuccessful", html_url);
				Err(Error::ChecksFailed {
					commit_sha: commit_sha.to_string(),
				}
				.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					pr.number,
				))))?;
			} else {
				log::info!("{} checks incomplete", html_url);
			}
		} else {
			// Head sha has changed since merge request.
			log::info!(
				"Head sha has changed since merge was requested on {}",
				html_url
			);
			Err(Error::HeadChanged {
				commit_sha: commit_sha.to_string(),
			}
			.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				pr.number,
			))))?;
		}
	}

	Ok(())
}

/// Parse bot commands in pull request comments.  Possible commands include:
/// `bot merge`
/// `bot merge force`
/// `bot merge cancel`
/// `bot compare substrate`
/// `bot rebase`
/// `bot burnin`
///
/// See also README.md.
async fn handle_comment(
	body: String,
	requested_by: String,
	number: i64,
	html_url: String,
	repo_url: String,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	let owner = GithubBot::owner_from_html_url(&html_url).context(Message {
		msg: format!("Failed parsing owner in url: {}", html_url),
	})?;

	let repo_name =
		repo_url.rsplit('/').next().map(|s| s.to_string()).context(
			Message {
				msg: format!("Failed parsing repo name in url: {}", repo_url),
			},
		)?;

	// Fetch the pr to get all fields (eg. mergeable).
	let pr = github_bot
		.pull_request(owner, &repo_name, number)
		.await
		.map_err(|e| {
			e.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				number,
			)))
		})?;

	let auth =
		GithubUserAuthenticator::new(&requested_by, owner, &repo_name, number);

	if body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim() {
		//
		// MERGE
		//
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		auth.check_org_membership(&github_bot).await?;

		//
		// merge allowed
		//
		merge_allowed(
			github_bot,
			owner,
			&repo_name,
			&pr,
			&bot_config,
			&requested_by,
		)
		.await?;

		//
		// status and merge
		//
		if ready_to_merge(github_bot, owner, &repo_name, &pr).await? {
			prepare_to_merge(
				github_bot,
				owner,
				&repo_name,
				pr.number,
				&pr.html_url,
			)
			.await?;

			merge(github_bot, owner, &repo_name, &pr).await?;
			update_companion(github_bot, &repo_name, &pr, db).await?;
		} else {
			wait_to_merge(
				github_bot,
				owner,
				&repo_name,
				pr.number,
				&pr.html_url,
				&requested_by,
				&pr.head.sha,
				db,
			)
			.await?;
		}
	} else if body.to_lowercase().trim()
		== AUTO_MERGE_FORCE.to_lowercase().trim()
	{
		//
		// MERGE
		//
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		auth.check_org_membership(&github_bot).await?;

		//
		// merge allowed
		//
		merge_allowed(
			github_bot,
			owner,
			&repo_name,
			&pr,
			&bot_config,
			&requested_by,
		)
		.await?;

		//
		// attempt merge without wait for checks
		//
		prepare_to_merge(
			github_bot,
			owner,
			&repo_name,
			pr.number,
			&pr.html_url,
		)
		.await?;
		merge(github_bot, owner, &repo_name, &pr).await?;
		update_companion(github_bot, &repo_name, &pr, db).await?;
	} else if body.to_lowercase().trim()
		== AUTO_MERGE_CANCEL.to_lowercase().trim()
	{
		//
		// CANCEL MERGE
		//
		log::info!(
			"Received merge cancel for PR {} from user {}",
			html_url,
			requested_by
		);
		log::info!("Deleting merge request for {}", &html_url);
		db.delete(pr.head.sha.trim().as_bytes())
			.context(Db)
			.map_err(|e| {
				e.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					number,
				)))
			})?;
		let _ = github_bot
			.create_issue_comment(
				owner,
				&repo_name,
				pr.number,
				"Merge cancelled.",
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
	} else if repo_name == "polkadot"
		&& body.to_lowercase().trim()
			== COMPARE_RELEASE_REQUEST.to_lowercase().trim()
	{
		//
		// DIFF
		//
		log::info!(
			"Received diff request for PR {} from user {}",
			html_url,
			requested_by
		);
		let rel = github_bot.latest_release(owner, &repo_name).await.map_err(
			|e| {
				e.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					number,
				)))
			},
		)?;
		let release_tag = github_bot
			.tag(owner, &repo_name, &rel.tag_name)
			.await
			.map_err(|e| {
				e.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					number,
				)))
			})?;
		let release_substrate_commit = github_bot
			.substrate_commit_from_polkadot_commit(&release_tag.object.sha)
			.await;
		let branch_substrate_commit = github_bot
			.substrate_commit_from_polkadot_commit(&pr.head.sha)
			.await;
		if release_substrate_commit.is_ok() && branch_substrate_commit.is_ok() {
			let link = github_bot.diff_url(
				owner,
				"substrate",
				&release_substrate_commit.unwrap(),
				&branch_substrate_commit.unwrap(),
			);

			// post link
			log::info!("Posting link to substrate diff: {}", &link);
			let _ = github_bot
				.create_issue_comment(owner, &repo_name, number, &link)
				.await
				.map_err(|e| {
					log::error!("Error posting comment: {}", e);
				});
		} else {
			if let Err(e) = release_substrate_commit {
				log::error!("Error getting release substrate commit: {}", e);
				Err(e.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					number,
				))))?;
			}
			if let Err(e) = branch_substrate_commit {
				log::error!("Error getting branch substrate commit: {}", e);
				Err(e.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					number,
				))))?;
			}
		}
	} else if body.to_lowercase().trim() == REBASE.to_lowercase().trim() {
		log::info!("Rebase {} requested by {}", html_url, requested_by);
		if let PullRequest {
			head:
				Head {
					ref_field: head_branch,
					repo:
						HeadRepo {
							name: head_repo,
							owner:
								Some(User {
									login: head_owner, ..
								}),
							..
						},
					..
				},
			..
		} = pr.clone()
		{
			let _ = github_bot
				.create_issue_comment(owner, &repo_name, pr.number, "Rebasing.")
				.await
				.map_err(|e| {
					log::error!("Error posting comment: {}", e);
				});
			rebase(
				github_bot,
				owner,
				&repo_name,
				&head_owner,
				&head_repo,
				&head_branch,
			)
			.await?;
		} else {
			Err(Error::Message {
				msg: format!(
					"PR response is missing required fields; rebase aborted."
				),
			}
			.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				number,
			))))?;
		}
	} else if body.to_lowercase().trim() == BURNIN_REQUEST.to_lowercase().trim()
	{
		auth.check_org_membership(github_bot).await?;

		handle_burnin_request(
			github_bot,
			&state.gitlab_bot,
			&state.matrix_bot,
			owner,
			&requested_by,
			&repo_name,
			&pr,
		)
		.await;
	}

	Ok(())
}

async fn handle_burnin_request(
	github_bot: &GithubBot,
	gitlab_bot: &GitlabBot,
	matrix_bot: &MatrixBot,
	owner: &str,
	requested_by: &str,
	repo_name: &str,
	pr: &PullRequest,
) {
	let make_job_link =
		|url| format!("<a href=\"{}\">CI job for burn-in deployment</a>", url);

	let unexpected_error_msg = "Starting CI job for burn-in deployment failed with an unexpected error; see logs.".to_string();
	let mut matrix_msg: Option<String> = None;

	let msg = match gitlab_bot.build_artifact(&pr.head.sha) {
		Ok(job) => {
			let ci_job_link = make_job_link(job.url);

			match job.status {
				JobStatus::Started => {
					format!("{} was started successfully.", ci_job_link)
				}
				JobStatus::AlreadyRunning => {
					format!("{} is already running.", ci_job_link)
				}
				JobStatus::Finished => format!(
					"{} already ran and finished with status `{}`.",
					ci_job_link, job.status_raw,
				),
				JobStatus::Unknown => format!(
					"{} has unexpected status `{}`.",
					ci_job_link, job.status_raw,
				),
			}
		}
		Err(e) => {
			log::error!("handle_burnin_request: {}", e);
			match e {
				Error::GitlabJobNotFound { commit_sha } => format!(
					"No matching CI job was found for commit `{}`",
					commit_sha
				),
				Error::StartingGitlabJobFailed { url, status, body } => {
					let ci_job_link = make_job_link(url);

					matrix_msg = Some(format!(
						"Starting {} failed with HTTP status {} and body: {}",
						ci_job_link, status, body,
					));

					format!(
						"Starting {} failed with HTTP status {}",
						ci_job_link, status,
					)
				}
				Error::GitlabApi {
					method,
					url,
					status,
					body,
				} => {
					matrix_msg = Some(format!(
						"Request {} {} failed with reponse status {} and body: {}",
						method, url, status, body,
					));

					unexpected_error_msg
				}
				_ => unexpected_error_msg,
			}
		}
	};

	if let Err(e) = github_bot
		.create_issue_comment(owner, &repo_name, pr.number, &msg)
		.await
	{
		log::error!("Error posting GitHub comment: {}", e);
	}

	let full_matrix_msg = format!(
		"Received burn-in request for <a href=\"{}\">{}#{}</a> from {}<br />\n{}",
		pr.html_url, repo_name, pr.number, requested_by, matrix_msg.unwrap_or(msg),
	);

	if let Err(e) = matrix_bot.send_html_to_default(&full_matrix_msg) {
		log::error!("Error sending Matrix message: {}", e);
	}
}

/// Check if the pull request is mergeable and approved.
async fn merge_allowed(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
	bot_config: &BotConfig,
	requested_by: &str,
) -> Result<()> {
	let mergeable = pr.mergeable.unwrap_or(false);
	if !mergeable {
		log::info!("{} is unmergeable", pr.html_url);
		Err(Error::Message {
			msg: format!("The PR is currently unmergeable."),
		}
		.map_issue(Some((
			owner.to_string(),
			repo_name.to_string(),
			pr.number,
		))))?;
	} else {
		log::info!("{} is mergeable.", pr.html_url);

		let team_leads = github_bot
			.team(owner, "substrateteamleads")
			.and_then(|team| github_bot.team_members(team.id))
			.await
			.unwrap_or_else(|e| {
				log::error!("Error getting core devs: {}", e);
				vec![]
			});

		if team_leads.iter().any(|lead| lead.login == requested_by) {
			//
			// MERGE ALLOWED
			//
			log::info!("{} merge requested by a team lead.", pr.html_url);
		} else {
			fn label_insubstantial(label: &&Label) -> bool {
				label.name.contains("insubstantial")
			}
			let min_reviewers =
				if pr.labels.iter().find(label_insubstantial).is_some() {
					1
				} else {
					bot_config.min_reviewers
				};
			let core_devs = github_bot
				.team(owner, "core-devs")
				.and_then(|team| github_bot.team_members(team.id))
				.await
				.unwrap_or_else(|e| {
					log::error!("Error getting core devs: {}", e);
					vec![]
				});
			let reviews =
				github_bot.reviews(&pr.url).await.unwrap_or_else(|e| {
					log::error!("Error getting reviews: {}", e);
					vec![]
				});
			let core_approved = reviews
				.iter()
				.filter(|r| {
					core_devs.iter().any(|u| u.login == r.user.login)
						&& r.state == Some(ReviewState::Approved)
				})
				.count() >= min_reviewers;
			let lead_approved = reviews
				.iter()
				.filter(|r| {
					team_leads.iter().any(|u| u.login == r.user.login)
						&& r.state == Some(ReviewState::Approved)
				})
				.count() >= 1;
			if core_approved || lead_approved {
				//
				// MERGE ALLOWED
				//
				log::info!("{} has core or team lead approval.", pr.html_url);
			} else {
				// get process info
				let process = process::get_process(
					github_bot, owner, repo_name, pr.number,
				)
				.await
				.map_err(|e| {
					Error::ProcessFile {
						source: Box::new(e),
					}
					.map_issue(Some((
						owner.to_string(),
						repo_name.to_string(),
						pr.number,
					)))
				})?;
				let owner_approved = reviews
					.iter()
					.sorted_by_key(|r| r.submitted_at)
					.rev()
					.find(|r| process.is_owner(&r.user.login))
					.map_or(false, |r| r.state == Some(ReviewState::Approved));

				let owner_requested = process.is_owner(&requested_by);

				if owner_approved || owner_requested {
					//
					// MERGE ALLOWED
					//
					log::info!("{} has owner approval.", pr.html_url);
				} else {
					if process.is_empty() {
						Err(Error::ProcessInfo {}.map_issue(Some((
							owner.to_string(),
							repo_name.to_string(),
							pr.number,
						))))?;
					} else {
						Err(Error::Approval {}.map_issue(Some((
							owner.to_string(),
							repo_name.to_string(),
							pr.number,
						))))?;
					}
				}
			}
		}
	}

	Ok(())
}

/// Query checks and statuses.
///
/// This function is used when a merge request is first received, to decide whether to store the
/// request and wait for checks -- if so they will later be handled by `checks_and_status`.
async fn ready_to_merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
) -> Result<bool> {
	//
	// status
	//
	match github_bot
		.status(owner, &repo_name, &pr.head.sha)
		.await
		.map_err(|e| {
			e.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				pr.number,
			)))
		})? {
		CombinedStatus {
			state: StatusState::Success,
			..
		} => {
			log::info!("{} is green.", pr.html_url);
			//
			// checks
			//
			let checks = github_bot
				.check_runs(&owner, &repo_name, &pr.head.sha)
				.await
				.map_err(|e| {
					e.map_issue(Some((
						owner.to_string(),
						repo_name.to_string(),
						pr.number,
					)))
				})?;
			log::info!("{:?}", checks);
			if checks
				.check_runs
				.iter()
				.all(|r| r.conclusion == Some("success".to_string()))
			{
				//
				// status/checks green
				//
				return Ok(true);
			} else if checks
				.check_runs
				.iter()
				.all(|r| r.status == "completed".to_string())
			{
				//
				// status/checks failure
				//
				log::info!("{} checks were unsuccessful.", pr.html_url);
				return Err(Error::ChecksFailed {
					commit_sha: pr.head.sha.clone(),
				}
				.map_issue(Some((
					owner.to_string(),
					repo_name.to_string(),
					pr.number,
				))));
			} else {
				//
				// status/checks pending
				//
				return Ok(false);
			}
		}
		CombinedStatus {
			state: StatusState::Pending,
			..
		} => {
			//
			// status/checks pending
			//
			return Ok(false);
		}
		CombinedStatus {
			state: StatusState::Failure,
			..
		} => {
			//
			// status/checks failure
			//
			log::info!("{} status failure.", pr.html_url);
			return Err(Error::ChecksFailed {
				commit_sha: pr.head.sha.to_string(),
			}
			.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				pr.number,
			))));
		}
		CombinedStatus {
			state: StatusState::Error,
			..
		} => {
			//
			// status/checks failure
			//
			log::info!("{} status error.", pr.html_url);
			return Err(Error::ChecksFailed {
				commit_sha: pr.head.sha.to_string(),
			}
			.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				pr.number,
			))));
		}
	}
}

/// Create a merge request object.
///
/// If this has been called, error handling must remove the db entry.
async fn create_merge_request(
	owner: &str,
	repo_name: &str,
	number: i64,
	html_url: &str,
	requested_by: &str,
	commit_sha: &str,
	db: &DB,
) -> Result<()> {
	let m = MergeRequest {
		owner: owner.to_string(),
		repo_name: repo_name.to_string(),
		number: number,
		html_url: html_url.to_string(),
		requested_by: requested_by.to_string(),
	};
	log::info!("Serializing merge request: {:?}", m);
	let bytes = bincode::serialize(&m).context(Bincode).map_err(|e| {
		e.map_issue(Some((owner.to_string(), repo_name.to_string(), number)))
	})?;
	log::info!("Writing merge request to db (head sha: {})", commit_sha);
	db.put(commit_sha.trim().as_bytes(), bytes)
		.context(Db)
		.map_err(|e| {
			e.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				number,
			)))
		})?;
	Ok(())
}

/// Create a merge request, add it to the database, and post a comment stating the merge is
/// pending.
async fn wait_to_merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	number: i64,
	html_url: &str,
	requested_by: &str,
	commit_sha: &str,
	db: &DB,
) -> Result<()> {
	log::info!("{} checks incomplete.", html_url);
	create_merge_request(
		owner,
		repo_name,
		number,
		html_url,
		requested_by,
		commit_sha,
		db,
	)
	.await?;
	log::info!("Waiting for commit status.");
	let _ = github_bot
		.create_issue_comment(
			owner,
			&repo_name,
			number,
			"Waiting for commit status.",
		)
		.await
		.map_err(|e| {
			log::error!("Error posting comment: {}", e);
		});
	Ok(())
}

/// Post a comment stating the merge will be attempted.
async fn prepare_to_merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	number: i64,
	html_url: &str,
) -> Result<()> {
	log::info!("{} checks successful; trying merge.", html_url);
	let _ = github_bot
		.create_issue_comment(owner, &repo_name, number, "Trying merge.")
		.await
		.map_err(|e| {
			log::error!("Error posting comment: {}", e);
		});
	Ok(())
}

/// Send a merge request.
async fn merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
) -> Result<()> {
	github_bot
		.merge_pull_request(owner, repo_name, pr.number, &pr.head.sha)
		.await
		.map_err(|e| {
			Error::Merge {
				source: Box::new(e),
				commit_sha: pr.head.sha.clone(),
			}
			.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				pr.number,
			)))
		})?;
	log::info!("{} merged successfully.", pr.html_url);
	Ok(())
}

#[allow(dead_code)]
async fn performance_regression(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
) -> Result<()> {
	let _ = github_bot
		.create_issue_comment(
			owner,
			&repo_name,
			pr.number,
			"Running performance regression.",
		)
		.await
		.map_err(|e| {
			log::error!("Error posting comment: {}", e);
		});
	if let PullRequest {
		head:
			Head {
				ref_field: head_branch,
				repo:
					HeadRepo {
						name: head_repo,
						owner: Some(User {
							login: head_owner, ..
						}),
						..
					},
				..
			},
		..
	} = pr.clone()
	{
		match performance::regression(
			github_bot,
			owner,
			&repo_name,
			&head_owner,
			&head_repo,
			&head_branch,
		)
		.await
		{
			Ok(Some(reg)) => {
				if reg > 2. {
					log::error!("Performance regression shows factor {} change in benchmark average.", reg);
					Err(Error::Message {
							msg: format!("Performance regression shows greater than 2x increase in benchmark average; aborting merge."),
						}
						.map_issue(Some((
							owner.to_string(),
							repo_name.to_string(),
							pr.number,
						))))?;
				}
			}
			Ok(None) => {
				log::error!("Failed to complete performance regression.");
				let _ = github_bot
					.create_issue_comment(owner, &repo_name, pr.number, "Failed to complete performance regression; see logs; continuing merge.")
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
			Err(e) => {
				log::error!("Error running performance regression: {}", e);
				let _ = github_bot
					.create_issue_comment(owner, &repo_name, pr.number, "Error running performance regression; see logs; continuing merge.")
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
		}
	}
	Ok(())
}

/// Check for a Polkadot companion and update it if found.
async fn update_companion(
	github_bot: &GithubBot,
	repo_name: &str,
	pr: &PullRequest,
	db: &DB,
) -> Result<()> {
	if repo_name == "substrate" {
		log::info!("Checking for companion.");
		if let Some(body) = &pr.body {
			// check for link in pr body
			if let Some((comp_html_url, comp_owner, comp_repo, comp_number)) =
				companion_parse(&body)
			{
				log::info!("Found companion {}", comp_html_url);
				let comp_pr = github_bot
					.pull_request(&comp_owner, &comp_repo, comp_number)
					.await
					.map_err(|e| {
						e.map_issue(Some((
							comp_owner.to_string(),
							comp_repo.to_string(),
							comp_number,
						)))
					})?;

				if let PullRequest {
					head:
						Head {
							ref_field: comp_head_branch,
							repo:
								HeadRepo {
									name: comp_head_repo,
									owner:
										Some(User {
											login: comp_head_owner,
											..
										}),
									..
								},
							..
						},
					..
				} = comp_pr.clone()
				{
					log::info!("Updating companion {}", comp_html_url);
					if let Some(updated_sha) = companion_update(
						github_bot,
						&comp_owner,
						&comp_repo,
						&comp_head_owner,
						&comp_head_repo,
						&comp_head_branch,
					)
					.await
					.map_err(|e| {
						Error::Companion {
							source: Box::new(e),
						}
						.map_issue(Some((
							comp_owner.to_string(),
							comp_repo.to_string(),
							comp_number,
						)))
					})? {
						log::info!(
							"Companion updated; waiting for checks on {}",
							comp_html_url
						);

						// wait for checks on the update commit
						wait_to_merge(
							github_bot,
							&comp_owner,
							&comp_repo,
							comp_pr.number,
							&comp_pr.html_url,
							&format!("parity-processbot[bot]"),
							&updated_sha,
							db,
						)
						.await?;
					} else {
						log::info!(
							"Failed updating companion {}",
							comp_html_url
						);

						Err(Error::Message {
							msg: format!("Failed updating substrate."),
						}
						.map_issue(Some((
							comp_owner.to_string(),
							comp_repo.to_string(),
							comp_number,
						))))?;
					}
				} else {
					Err(Error::Companion {
						source: Box::new(Error::Message {
							msg: format!(
								"Companion PR is missing required fields."
							),
						}),
					}
					.map_issue(Some((
						comp_owner.to_string(),
						comp_repo.to_string(),
						comp_number,
					))))?;
				}
			} else {
				log::info!("No companion found.");
			}
		} else {
			log::info!("No PR body found.");
		}
	}

	Ok(())
}

/// Distinguish required statuses.
#[allow(dead_code)]
fn status_failure_allowed(ci: &str, context: &str) -> bool {
	let d: serde_yaml::Result<serde_yaml::Value> = serde_yaml::from_str(ci);
	match d {
		Ok(v) => v[context]["allow_failure"].as_bool().unwrap_or(false),
		Err(e) => {
			log::error!("Error parsing value from ci yaml: {}", e);
			false
		}
	}
}

const TROUBLESHOOT_MSG: &str = "Merge can be attempted if:\n- The PR has approval from two core-devs (or one if the PR is labelled insubstantial).\n- The PR has approval from a member of `substrateteamleads`.\n- The PR is attached to a project column and has approval from the project owner.\n\nSee https://github.com/paritytech/parity-processbot#faq";

async fn handle_error(e: Error, state: &AppState) {
	log::error!("{}", e);
	match e {
		Error::WithIssue {
			source,
			issue: Some((owner, repo, number)),
			..
		} => {
			let msg = match *source {
				Error::Companion { source } => {
					format!("Error updating substrate: {}", *source)
				}
				Error::Merge { source, commit_sha } => {
					// clean db
					let _ =
						state.db.delete(commit_sha.as_bytes()).map_err(|e| {
							log::error!(
								"Error deleting merge request from db: {}",
								e
							);
						});
					match *source {
						Error::Response {
							body: serde_json::Value::Object(m),
							..
						} => format!("Merge failed: `{}`", m["message"]),
						Error::Http { source, .. } => format!(
							"Merge failed due to network error:\n\n{}",
							source
						),
						e => format!(
							"Merge failed due to unexpected error:\n\n{}",
							e
						),
					}
				}
				Error::ProcessFile { source } => {
					match *source {
						Error::Response {
							body: serde_json::Value::Object(m),
							..
						} => format!("Error getting Process.json: `{}`", m["message"]),
						Error::Http { source, .. } => format!(
							"Network error getting Process.json:\n\n{}",
							source
						),
						e => format!(
							"Unexpected error getting Process.json:\n\n{}",
							e
						),
					}
				}
				Error::ProcessInfo { } => {
					format!("Missing process info; check that the PR belongs to a project column.\n\n{}", TROUBLESHOOT_MSG)
				}
				Error::Approval { } => {
					format!("Missing approval from the project owner or a minimum of core developers.\n\n{}", TROUBLESHOOT_MSG)
				}
				Error::HeadChanged { commit_sha } => {
					// clean db
					let _ =
						state.db.delete(commit_sha.as_bytes()).map_err(|e| {
							log::error!(
								"Error deleting merge request from db: {}",
								e
							);
						});
					format!("Head SHA changed; merge aborted.")
				}
				Error::ChecksFailed { commit_sha } => {
					// clean db
					let _ =
						state.db.delete(commit_sha.as_bytes()).map_err(|e| {
							log::error!(
								"Error deleting merge request from db: {}",
								e
							);
						});
					format!("Checks failed; merge aborted.")
				}
				Error::OrganizationMembership { source } => {
					format!("Error getting organization membership: {}", source)
				}
				Error::Message { msg } => format!("{}", msg),
				Error::Response {
					body: serde_json::Value::Object(m),
					..
				} => format!("Error: `{}`", m["message"]),
				_ => "Unexpected error; see logs.".to_string(),
			};
			let _ = state
				.github_bot
				.create_issue_comment(&owner, &repo, number, &msg)
				.await
				.map_err(|e| {
					log::error!("Error posting comment: {}", e);
				});
		}
		_ => {}
	}
}
