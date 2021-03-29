use futures::StreamExt;
use futures_util::future::TryFutureExt;
use hyper::{http::StatusCode, Body, Request, Response};
use itertools::Itertools;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
	auth::GithubUserAuthenticator, companion::*, config::BotConfig,
	constants::*, error::*, github::*, github_bot::GithubBot, gitlab_bot::*,
	matrix_bot::MatrixBot, performance, process, rebase::*, Result, Status,
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

	log::info!("Parsing payload {}", String::from_utf8_lossy(&msg_bytes));
	match serde_json::from_slice::<Payload>(&msg_bytes) {
		Ok(payload) => handle_payload(payload, state).await,
		Err(err) => {
			// If this comment was originated from a Bot, then acting on it might make the bot
			// to respond to itself recursively, as happened on
			// https://github.com/paritytech/substrate/pull/8409. Therefore we'll only act on
			// this error if it's known for sure it has been initiated only by a User comment.
			let pr_details = serde_json::from_slice::<
				DetectUserCommentPullRequest,
			>(&msg_bytes)
			.ok()
			.map(|detected| detected.get_issue_details())
			.flatten();

			if let Some(pr_details) = pr_details {
				Err(Error::Message {
					msg: format!(
						"Webhook event parsing failed due to:

```
{}
```

Payload:

```
{}
```
                ",
						err.to_string(),
						String::from_utf8_lossy(&msg_bytes)
					),
				}
				.map_issue(pr_details))
			} else {
				log::info!("Ignoring payload parsing error",);
				Ok(())
			}
		}
	}
}

/// Match different kinds of payload.
async fn handle_payload(payload: Payload, state: &AppState) -> Result<()> {
	match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			comment:
				Comment {
					body,
					user:
						Some(User {
							login,
							type_field: Some(UserType::User),
							..
						}),
					..
				},
			issue,
		} => match &issue {
			Issue {
				number,
				html_url,
				repository_url: Some(repo_url),
				pull_request: Some(_),
				..
			} => {
				handle_comment(body, &login, *number, html_url, repo_url, state)
					.await
					.map_err(|e| match e {
						Error::WithIssue { .. } => e,
						e => {
							if let Some(details) = issue.get_issue_details() {
								e.map_issue(details)
							} else {
								e
							}
						}
					})
			}
			_ => Ok(()),
		},
		Payload::CommitStatus {
			sha, state: status, ..
		} => handle_status(sha, status, state).await,
		Payload::CheckRun {
			check_run: CheckRun {
				status, head_sha, ..
			},
			..
		} => handle_check(status, head_sha, state).await,
		_ => Ok(()),
	}
}

/// If a check completes, query if all statuses and checks are complete.
async fn handle_check(
	status: CheckRunStatus,
	commit_sha: String,
	state: &AppState,
) -> Result<()> {
	if status == CheckRunStatus::Completed {
		checks_and_status(&state.github_bot, &state.db, &commit_sha).await
	} else {
		Ok(())
	}
}

/// If we receive a status other than `Pending`, query if all statuses and checks are complete.
async fn handle_status(
	commit_sha: String,
	status: StatusState,
	state: &AppState,
) -> Result<()> {
	if status == StatusState::Pending {
		Ok(())
	} else {
		checks_and_status(&state.github_bot, &state.db, &commit_sha).await
	}
}

async fn get_latest_statuses_state(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let status = github_bot.status(&owner, &owner_repo, &commit_sha).await?;
	log::info!("{:?}", status);

	// Since Github only considers the latest instance of each status, we should abide by the same
	// rule. Each instance is uniquely identified by "context".
	let mut latest_statuses: HashMap<String, (i64, StatusState)> =
		HashMap::new();
	for s in status.statuses {
		if latest_statuses
			.get(&s.context)
			.map(|(prev_id, _)| prev_id < &(&s).id)
			.unwrap_or(true)
		{
			latest_statuses.insert(s.context, (s.id, s.state));
		}
	}

	Ok(
		if latest_statuses
			.values()
			.all(|(_, state)| *state == StatusState::Success)
		{
			log::info!("{} has success status", html_url);
			Status::Success
		} else if latest_statuses
			.values()
			.any(|(_, state)| *state == StatusState::Pending)
		{
			log::info!("{} has pending status", html_url);
			Status::Pending
		} else {
			log::info!("{} has failed status", html_url);
			Status::Failure
		},
	)
}

async fn get_latest_checks_state(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let checks = github_bot
		.check_runs(&owner, &repo_name, commit_sha)
		.await?;
	log::info!("{:?}", checks);

	// Since Github only considers the latest instance of each status, we should abide by the same
	// rule. Each instance is uniquely identified by "context".
	let mut latest_checks: HashMap<
		String,
		(i64, CheckRunStatus, Option<CheckRunConclusion>),
	> = HashMap::new();
	for c in checks.check_runs {
		if latest_checks
			.get(&c.name)
			.map(|(prev_id, _, _)| prev_id < &(&c).id)
			.unwrap_or(true)
		{
			latest_checks.insert(c.name, (c.id, c.status, c.conclusion));
		}
	}

	Ok(
		if latest_checks.values().all(|(_, _, conclusion)| {
			*conclusion == Some(CheckRunConclusion::Success)
		}) {
			log::info!("{} has successful checks", html_url);
			Status::Success
		} else if latest_checks
			.values()
			.all(|(_, status, _)| *status == CheckRunStatus::Completed)
		{
			log::info!("{} has unsuccessful checks", html_url);
			Status::Failure
		} else {
			log::info!("{} has pending checks", html_url);
			Status::Pending
		},
	)
}

/// Check that no commit has been pushed since the merge request was received.  Query checks and
/// statuses and if they are green, attempt merge.
async fn checks_and_status(
	github_bot: &GithubBot,
	db: &DB,
	commit_sha: &str,
) -> Result<()> {
	if let Some(pr_bytes) = db.get(commit_sha.as_bytes()).context(Db)? {
		let m = bincode::deserialize(&pr_bytes).context(Bincode)?;
		log::info!("Deserialized merge request: {:?}", m);
		let MergeRequest {
			owner,
			repo_name,
			number,
			html_url,
			..
		} = m;

		match github_bot.pull_request(&owner, &repo_name, number).await {
			Ok(pr) => {
				match get_latest_statuses_state(
					github_bot, &owner, &repo_name, commit_sha, &html_url,
				)
				.await
				{
					Ok(status) => match status {
						Status::Success => {
							match get_latest_checks_state(
								github_bot, &owner, &repo_name, commit_sha,
								&html_url,
							)
							.await
							{
								Ok(status) => match status {
									Status::Success => {
										merge(
											github_bot, &owner, &repo_name, &pr,
										)
										.await?;
										db.delete(&commit_sha).context(Db)?;
										update_companion(
											github_bot, &repo_name, &pr, db,
										)
										.await
									}
									Status::Failure => {
										Err(Error::ChecksFailed {
											commit_sha: commit_sha.to_string(),
										})
									}
									_ => Ok(()),
								},
								Err(e) => Err(e),
							}
						}
						Status::Failure => Err(Error::ChecksFailed {
							commit_sha: commit_sha.to_string(),
						}),
						_ => Ok(()),
					},
					Err(e) => Err(e),
				}
			}
			Err(e) => Err(e),
		}
		.map_err(|e| e.map_issue((owner, repo_name, number)))?;
	}

	Ok(())
}

/// Parse bot commands in pull request comments. Commands are listed in README.md.
async fn handle_comment(
	body: String,
	requested_by: &str,
	number: i64,
	html_url: &str,
	repo_url: &str,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	let owner = GithubBot::owner_from_html_url(html_url).context(Message {
		msg: format!("Failed parsing owner in url: {}", html_url),
	})?;

	let repo_name =
		repo_url.rsplit('/').next().map(|s| s.to_string()).context(
			Message {
				msg: format!("Failed parsing repo name in url: {}", repo_url),
			},
		)?;

	// Fetch the pr to get all fields (eg. mergeable).
	let pr = &github_bot.pull_request(owner, &repo_name, number).await?;

	let auth =
		GithubUserAuthenticator::new(requested_by, owner, &repo_name, number);

	if body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim() {
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		auth.check_org_membership(&github_bot).await?;

		merge_allowed(
			github_bot,
			owner,
			&repo_name,
			pr,
			&bot_config,
			requested_by,
		)
		.await?;

		if ready_to_merge(github_bot, owner, &repo_name, pr).await? {
			prepare_to_merge(
				github_bot,
				owner,
				&repo_name,
				pr.number,
				&pr.html_url,
			)
			.await?;

			merge(github_bot, owner, &repo_name, pr).await?;
			update_companion(github_bot, &repo_name, pr, db).await?;
		} else {
			let pr_head_sha = pr.head_sha()?;
			wait_to_merge(
				github_bot,
				owner,
				&repo_name,
				pr.number,
				&pr.html_url,
				requested_by,
				pr_head_sha,
				db,
			)
			.await?;
		}
	} else if body.to_lowercase().trim()
		== AUTO_MERGE_FORCE.to_lowercase().trim()
	{
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		auth.check_org_membership(&github_bot).await?;

		merge_allowed(
			github_bot,
			owner,
			&repo_name,
			&pr,
			&bot_config,
			requested_by,
		)
		.await?;

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
		let pr_head_sha = pr.head_sha()?;

		log::info!(
			"Received merge cancel for PR {} from user {}",
			html_url,
			requested_by
		);
		log::info!("Deleting merge request for {}", html_url);
		db.delete(pr_head_sha.as_bytes()).context(Db)?;
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
		let pr_head_sha = pr.head_sha()?;

		log::info!(
			"Received diff request for PR {} from user {}",
			html_url,
			requested_by
		);
		let rel = github_bot.latest_release(owner, &repo_name).await?;
		let release_tag =
			github_bot.tag(owner, &repo_name, &rel.tag_name).await?;
		let release_substrate_commit = github_bot
			.substrate_commit_from_polkadot_commit(&release_tag.object.sha)
			.await?;
		let branch_substrate_commit = github_bot
			.substrate_commit_from_polkadot_commit(pr_head_sha)
			.await?;
		let link = github_bot.diff_url(
			owner,
			"substrate",
			&release_substrate_commit,
			&branch_substrate_commit,
		);

		log::info!("Posting link to substrate diff: {}", &link);
		let _ = github_bot
			.create_issue_comment(owner, &repo_name, number, &link)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
	} else if body.to_lowercase().trim() == REBASE.to_lowercase().trim() {
		log::info!("Rebase {} requested by {}", html_url, requested_by);
		{
			if let PullRequest {
				head:
					Some(Head {
						ref_field: Some(head_branch),
						repo:
							Some(HeadRepo {
								name: head_repo,
								owner:
									Some(User {
										login: head_owner, ..
									}),
								..
							}),
						..
					}),
				..
			} = pr.clone()
			{
				let _ = github_bot
					.create_issue_comment(
						owner,
						&repo_name,
						pr.number,
						"Rebasing.",
					)
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
				.await
			} else {
				Err(Error::Message {
					msg: format!("PR is missing some API data"),
				})
			}
		}
		.map_err(|e| Error::Rebase {
			source: Box::new(e),
		})?;
	} else if body.to_lowercase().trim() == BURNIN_REQUEST.to_lowercase().trim()
	{
		auth.check_org_membership(github_bot).await?;

		handle_burnin_request(
			github_bot,
			&state.gitlab_bot,
			&state.matrix_bot,
			owner,
			requested_by,
			&repo_name,
			&pr,
		)
		.await?;
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
) -> Result<()> {
	let make_job_link =
		|url| format!("<a href=\"{}\">CI job for burn-in deployment</a>", url);

	let unexpected_error_msg = "Starting CI job for burn-in deployment failed with an unexpected error; see logs.".to_string();
	let mut matrix_msg: Option<String> = None;

	let pr_head_sha = pr.head_sha()?;

	let msg = match gitlab_bot.build_artifact(pr_head_sha) {
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

	github_bot
		.create_issue_comment(owner, &repo_name, pr.number, &msg)
		.await?;

	matrix_bot.send_html_to_default(
		format!(
		"Received burn-in request for <a href=\"{}\">{}#{}</a> from {}<br />\n{}",
		pr.html_url, repo_name, pr.number, requested_by, matrix_msg.unwrap_or(msg),
	)
		.as_str(),
	)?;

	Ok(())
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
		let msg = format!("{} is unmergeable", pr.html_url);
		log::info!("{}", msg);
		return Err(Error::Message { msg });
	}

	log::info!("{} is mergeable.", pr.html_url);

	let team_leads = github_bot
		.team(owner, SUBSTRATE_TEAM_LEADS_GROUP)
		.and_then(|team| github_bot.team_members(team.id))
		.await
		.unwrap_or_else(|e| {
			log::error!("Error getting {}: {}", SUBSTRATE_TEAM_LEADS_GROUP, e);
			vec![]
		});

	if team_leads.iter().any(|lead| lead.login == requested_by) {
		log::info!("{} merge requested by a team lead.", pr.html_url);
	} else {
		let min_reviewers = if pr
			.labels
			.iter()
			.find(|label| label.name.contains("insubstantial"))
			.is_some()
		{
			1
		} else {
			bot_config.min_reviewers
		};
		let core_devs = github_bot
			.team(owner, CORE_DEVS_GROUP)
			.and_then(|team| github_bot.team_members(team.id))
			.await
			.unwrap_or_else(|e| {
				log::error!("Error getting {}: {}", CORE_DEVS_GROUP, e);
				vec![]
			});
		let reviews = github_bot.reviews(&pr.url).await.unwrap_or_else(|e| {
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
			log::info!("{} has core or team lead approval.", pr.html_url);
		} else {
			let process =
				process::get_process(github_bot, owner, repo_name, pr.number)
					.await
					.map_err(|e| Error::ProcessFile {
						source: Box::new(e),
					})?;

			let owner_approved = reviews
				.iter()
				.sorted_by_key(|r| r.submitted_at)
				.rev()
				.find(|r| process.is_owner(&r.user.login))
				.map_or(false, |r| r.state == Some(ReviewState::Approved));
			let owner_requested = process.is_owner(&requested_by);

			if owner_approved || owner_requested {
				log::info!("{} has owner approval.", pr.html_url);
			} else if process.is_empty() {
				return Err(Error::ProcessInfo {});
			} else {
				return Err(Error::Approval {});
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
	match pr.head_sha() {
		Ok(pr_head_sha) => {
			match get_latest_statuses_state(
				github_bot,
				owner,
				repo_name,
				pr_head_sha,
				&pr.html_url,
			)
			.await
			{
				Ok(status) => match status {
					Status::Success => {
						match get_latest_checks_state(
							github_bot,
							owner,
							repo_name,
							pr_head_sha,
							&pr.html_url,
						)
						.await
						{
							Ok(status) => match status {
								Status::Success => Ok(true),
								Status::Failure => Err(Error::ChecksFailed {
									commit_sha: pr_head_sha.to_string(),
								}),
								_ => Ok(false),
							},
							Err(e) => Err(e),
						}
					}
					Status::Failure => Err(Error::ChecksFailed {
						commit_sha: pr_head_sha.to_string(),
					}),
					_ => Ok(false),
				},
				Err(e) => Err(e),
			}
		}
		Err(e) => Err(e),
	}
	.map_err(|e| {
		e.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
	})
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
		e.map_issue((owner.to_string(), repo_name.to_string(), number))
	})?;
	log::info!("Writing merge request to db (head sha: {})", commit_sha);
	db.put(commit_sha.trim().as_bytes(), bytes)
		.context(Db)
		.map_err(|e| {
			e.map_issue((owner.to_string(), repo_name.to_string(), number))
		})?;
	Ok(())
}

/// Create a merge request, add it to the database, and post a comment stating the merge is
/// pending.
pub async fn wait_to_merge(
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
	let pr_head_sha = pr.head_sha()?;
	github_bot
		.merge_pull_request(owner, repo_name, pr.number, pr_head_sha)
		.await
		.map_err(|e| {
			Error::Merge {
				source: Box::new(e),
				commit_sha: pr_head_sha.to_string(),
			}
			.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
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
			Some(Head {
				ref_field: Some(head_branch),
				repo:
					Some(HeadRepo {
						name: head_repo,
						owner: Some(User {
							login: head_owner, ..
						}),
						..
					}),
				..
			}),
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
						.map_issue((
							owner.to_string(),
							repo_name.to_string(),
							pr.number,
						)))?;
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

const TROUBLESHOOT_MSG: &str = "Merge cannot succeed as it is. Check out the [criteria for merge](https://github.com/paritytech/parity-processbot#criteria-for-merge).";

fn handle_error_inner(err: Error, state: &AppState) -> Option<String> {
	match err {
		Error::Merge { source, commit_sha } => {
			let _ = state.db.delete(commit_sha.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			match *source {
				Error::Response {
					body: serde_json::Value::Object(m),
					..
				} => Some(format!("Merge failed: `{}`", m["message"])),
				Error::Http { source, .. } => {
					Some(format!("Merge failed due to network error:\n\n{}", source))
				}
				_ => Some(format!("Merge failed due to unexpected error:\n\n{}", source)),
			}
		}
		Error::ProcessFile { source } => match *source {
			Error::Response {
				body: serde_json::Value::Object(m),
				..
			} => Some(format!("Error getting Process.json: `{}`", m["message"])),
			Error::Http { source, .. } => {
				Some(format!("Network error getting Process.json:\n\n{}", source))
			}
			_ => Some(format!("Unexpected error getting Process.json:\n\n{}", source)),
		},
		Error::ProcessInfo {} => {
			Some(format!("Missing process info; check that the PR belongs to a project column.\n\n{}", TROUBLESHOOT_MSG))
		}
		Error::Approval {} => {
			Some(format!("Missing approval from the project owner or a minimum of core developers.\n\n{}", TROUBLESHOOT_MSG))
		}
		Error::HeadChanged { ref expected, .. } => {
			let _ = state.db.delete(expected.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			Some(format!("Merge aborted: {}", err))
		}
		Error::ChecksFailed { ref commit_sha } => {
			let _ = state.db.delete(commit_sha.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			Some(format!("Merge aborted: {}", err))
		}
		Error::Response {
			body: serde_json::Value::Object(m),
			..
		} => Some(format!("Response error: `{}`", m["message"])),
		Error::OrganizationMembership { .. }
		| Error::CompanionUpdate { .. }
		| Error::Message { .. }
		| Error::Rebase { .. } => {
			Some(format!("Error: {}", err))
		}
		_ => None
	}
}

async fn handle_error(e: Error, state: &AppState) {
	log::error!("handle_error: {}", e);

	match e {
		Error::WithIssue {
			source,
			issue: (owner, repo, number),
			..
		} => {
			let msg = handle_error_inner(*source, state).unwrap_or_else(|| {
				format!(
					"Unexpected error (at {} server time).",
					chrono::Utc::now().to_string()
				)
			});
			let _ = state
				.github_bot
				.create_issue_comment(&owner, &repo, number, &msg)
				.await
				.map_err(|e| {
					log::error!("Error posting comment: {}", e);
				});
		}
		_ => {
			handle_error_inner(e, state);
		}
	}
}
