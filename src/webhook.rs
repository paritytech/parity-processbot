use async_recursion::async_recursion;
use futures::StreamExt;
use hyper::{Body, Request, Response, StatusCode};
use itertools::Itertools;
use regex::RegexBuilder;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use crate::{
	auth::GithubUserAuthenticator, companion::*, config::BotConfig,
	config::MainConfig, constants::*, error::*, github::*,
	github_bot::GithubBot, gitlab_bot::*, matrix_bot::MatrixBot, performance,
	process, rebase::*, results, utils::*, Result, Status,
};

/// This data gets passed along with each webhook to the webhook handler.
pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub gitlab_bot: GitlabBot,

	pub bot_config: BotConfig,
	pub config: MainConfig,
}

/// This stores information about a pull request while we wait for checks to complete.
#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	pub contributor: String,
	pub contributor_repo: String,
	pub owner: String,
	pub owner_repo: String,
	pub number: i64,
	pub html_url: String,
	pub requested_by: String,
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
		state.config.webhook_secret.trim().as_bytes(),
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
pub async fn handle_payload(payload: Payload, state: &AppState) -> Result<()> {
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
		Payload::CommitStatus { sha, state: status } => {
			handle_status(sha, status, state).await
		}
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
		checks_and_status(
			&state.bot_config,
			&state.github_bot,
			&state.db,
			&commit_sha,
		)
		.await
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
		checks_and_status(
			&state.bot_config,
			&state.github_bot,
			&state.db,
			&commit_sha,
		)
		.await
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

	// Since Github only considers the latest instance of each check, we should abide by the same
	// rule. Each instance is uniquely identified by "name".
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
	bot_config: &BotConfig,
	github_bot: &GithubBot,
	db: &DB,
	commit_sha: &str,
) -> Result<()> {
	if let Some(pr_bytes) = db.get(commit_sha.as_bytes()).context(Db)? {
		let m = bincode::deserialize(&pr_bytes).context(Bincode)?;
		log::info!("Deserialized merge request: {:?}", m);
		let MergeRequest {
			contributor,
			contributor_repo,
			owner,
			owner_repo,
			number,
			html_url,
			requested_by,
		} = m;

		// Wait a bit for all the statuses to settle; some missing status might be
		// delivered with a small delay right after this is triggered, thus it's
		// worthwhile to wait for it instead of having to recover from a premature
		// merge attempt due to some slightly-delayed missing status.
		tokio::time::delay_for(std::time::Duration::from_millis(2048)).await;

		match github_bot
			.pull_request(&contributor, &contributor_repo, number)
			.await
		{
			Ok(pr) => match results!(pr.head_sha(), pr.head_ref()) {
				Ok((head_sha, contributor_branch)) => {
					if commit_sha != head_sha {
						Err(Error::HeadChanged {
							expected: commit_sha.to_string(),
							actual: head_sha.to_owned(),
						})
					} else {
						match get_latest_statuses_state(
							github_bot,
							&contributor,
							&contributor_repo,
							commit_sha,
							&html_url,
						)
						.await
						{
							Ok(status) => match status {
								Status::Success => {
									match get_latest_checks_state(
										github_bot,
										&contributor,
										&contributor_repo,
										commit_sha,
										&html_url,
									)
									.await
									{
										Ok(status) => match status {
											Status::Success => {
												merge(
													github_bot,
													bot_config,
													&contributor,
													&contributor_repo,
													contributor_branch,
													&owner,
													&owner_repo,
													head_sha,
													&pr,
													&requested_by,
													None,
												)
												.await??;
												db.delete(&commit_sha)
													.context(Db)?;
												update_companion(
													github_bot,
													&contributor_repo,
													&pr,
													db,
												)
												.await
											}
											Status::Failure => {
												Err(Error::ChecksFailed {
													commit_sha: commit_sha
														.to_string(),
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
				}
				Err(e) => Err(e),
			},
			Err(e) => Err(e),
		}
		.map_err(|e| e.map_issue((contributor, contributor_repo, number)))?;
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

	let contributor =
		GithubBot::owner_from_html_url(html_url).context(Message {
			msg: format!("Failed parsing contributor in url: {}", html_url),
		})?;

	let contributor_repo =
		repo_url.rsplit('/').next().map(|s| s.to_string()).context(
			Message {
				msg: format!("Failed parsing repo name in url: {}", repo_url),
			},
		)?;

	// Fetch the pr to get all fields (eg. mergeable).
	let pr = &github_bot
		.pull_request(contributor, &contributor_repo, number)
		.await?;
	let head_sha = pr.head_sha()?;
	let contributor_branch = pr.head_ref()?;
	let owner = pr.base_owner()?;
	let owner_repo = pr.base_name()?;

	GithubUserAuthenticator::new(
		requested_by,
		contributor,
		&contributor_repo,
		number,
	)
	.check_org_membership(&github_bot)
	.await?;

	let is_merge =
		body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim();
	let is_merge_forced =
		body.to_lowercase().trim() == AUTO_MERGE_FORCE.to_lowercase().trim();

	if is_merge || is_merge_forced {
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		merge_allowed(
			github_bot,
			contributor,
			&contributor_repo,
			pr,
			bot_config,
			requested_by,
			None,
		)
		.await??;

		if is_merge_forced
			|| ready_to_merge(github_bot, contributor, &contributor_repo, pr)
				.await?
		{
			prepare_to_merge(
				github_bot,
				contributor,
				&contributor_repo,
				pr.number,
				&pr.html_url,
			)
			.await?;
			merge(
				github_bot,
				bot_config,
				owner,
				owner_repo,
				contributor,
				&contributor_repo,
				contributor_branch,
				head_sha,
				pr,
				requested_by,
				None,
			)
			.await??;
			update_companion(github_bot, &contributor_repo, pr, db).await?;
		} else {
			wait_to_merge(
				github_bot,
				db,
				head_sha,
				MergeRequest {
					contributor: contributor.to_string(),
					contributor_repo,
					owner: owner.to_string(),
					owner_repo: owner_repo.to_string(),
					number,
					html_url: html_url.to_string(),
					requested_by: html_url.to_string(),
				},
			)
			.await?;
		}
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
				contributor,
				&contributor_repo,
				pr.number,
				"Merge cancelled.",
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
	} else if contributor_repo == "polkadot"
		&& body.to_lowercase().trim()
			== COMPARE_RELEASE_REQUEST.to_lowercase().trim()
	{
		let pr_head_sha = pr.head_sha()?;

		log::info!(
			"Received diff request for PR {} from user {}",
			html_url,
			requested_by
		);
		let rel = github_bot
			.latest_release(contributor, &contributor_repo)
			.await?;
		let release_tag = github_bot
			.tag(contributor, &contributor_repo, &rel.tag_name)
			.await?;
		let release_substrate_commit = github_bot
			.substrate_commit_from_polkadot_commit(&release_tag.object.sha)
			.await?;
		let branch_substrate_commit = github_bot
			.substrate_commit_from_polkadot_commit(pr_head_sha)
			.await?;
		let link = github_bot.diff_url(
			contributor,
			"substrate",
			&release_substrate_commit,
			&branch_substrate_commit,
		);

		log::info!("Posting link to substrate diff: {}", &link);
		let _ = github_bot
			.create_issue_comment(contributor, &contributor_repo, number, &link)
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
						contributor,
						&contributor_repo,
						pr.number,
						"Rebasing.",
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
				rebase(
					github_bot,
					contributor,
					&contributor_repo,
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
		handle_burnin_request(
			github_bot,
			&state.gitlab_bot,
			&state.matrix_bot,
			contributor,
			requested_by,
			&contributor_repo,
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
/// Errors related to core-devs and substrateteamleads API requests are ignored
/// because the merge might succeed regardless of them, thus it does not make
/// sense to fail this scenario completely if the request fails for some reason.
async fn merge_allowed(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
	bot_config: &BotConfig,
	requested_by: &str,
	min_approvals_required: Option<usize>,
) -> Result<Result<Option<String>>> {
	let is_mergeable = pr.mergeable.unwrap_or(false);

	if let Some(min_approvals_required) = &min_approvals_required {
		log::info!(
			"Attempting to reach minimum number of approvals {}",
			min_approvals_required
		);
	} else if is_mergeable {
		log::info!("{} is mergeable", pr.html_url);
	} else {
		log::info!("{} is not mergeable", pr.html_url);
	}

	if is_mergeable || min_approvals_required.is_some() {
		match github_bot.reviews(&pr.url).await {
			Ok(reviews) => {
				let mut errors: Vec<String> = Vec::new();

				// Consider only the latest relevant review submitted per user
				let mut latest_reviews: HashMap<String, (i64, Review)> =
					HashMap::new();
				for review in reviews {
					// Do not consider states such as "Commented" as having invalidated a previous
					// approval. Note: this assumes approvals are not invalidated on comments or
					// pushes.
					if review
						.state
						.as_ref()
						.map(|state| {
							*state == ReviewState::Approved
								|| *state == ReviewState::ChangesRequested
						})
						.unwrap_or(false)
					{
						if let Some(user) = review.user.as_ref() {
							if latest_reviews
								.get(&user.login)
								.map(|(prev_id, _)| *prev_id < review.id)
								.unwrap_or(true)
							{
								let user_login = (&user.login).to_owned();
								latest_reviews
									.insert(user_login, (review.id, review));
							}
						}
					}
				}
				let approved_reviews = latest_reviews
					.values()
					.filter_map(|(_, review)| {
						if review.state == Some(ReviewState::Approved) {
							Some(review)
						} else {
							None
						}
					})
					.collect::<Vec<_>>();

				let team_leads = github_bot
					.substrate_team_leads(owner)
					.await
					.unwrap_or_else(|e| {
						let msg = format!(
							"Error getting {}: `{}`",
							SUBSTRATE_TEAM_LEADS_GROUP, e
						);
						log::error!("{}", msg);
						errors.push(msg);
						vec![]
					});

				let is_allowed = if team_leads
					.iter()
					.any(|lead| lead.login == requested_by)
				{
					log::info!(
						"{} merge requested by a team lead.",
						pr.html_url
					);
					Ok(())
				} else {
					let core_devs =
						github_bot.core_devs(owner).await.unwrap_or_else(|e| {
							let msg = format!(
								"Error getting {}: `{}`",
								CORE_DEVS_GROUP, e
							);
							log::error!("{}", msg);
							errors.push(msg);
							vec![]
						});

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

					let core_approved = approved_reviews
						.iter()
						.filter(|review| {
							core_devs.iter().any(|core_dev| {
								review
									.user
									.as_ref()
									.map(|user| user.login == core_dev.login)
									.unwrap_or(false)
							})
						})
						.count() >= min_reviewers;
					let lead_approved = approved_reviews
						.iter()
						.filter(|review| {
							team_leads.iter().any(|team_lead| {
								review
									.user
									.as_ref()
									.map(|user| user.login == team_lead.login)
									.unwrap_or(false)
							})
						})
						.count() >= 1;

					if core_approved || lead_approved {
						log::info!(
							"{} has core or team lead approval.",
							pr.html_url
						);
						Ok(())
					} else {
						match process::get_process(
							github_bot, owner, repo_name, pr.number,
						)
						.await
						{
							Ok((process, process_warnings)) => {
								let project_owner_approved = approved_reviews
									.iter()
									.rev()
									.any(|review| {
										review
											.user
											.as_ref()
											.map(|user| {
												process.is_owner(&user.login)
											})
											.unwrap_or(false)
									});
								let project_owner_requested =
									process.is_owner(requested_by);

								if project_owner_approved
									|| project_owner_requested
								{
									log::info!(
										"{} has project owner approval.",
										pr.html_url
									);
									Ok(())
								} else {
									errors.extend(process_warnings);
									if process.is_empty() {
										Err(Error::ProcessInfo {
											errors: Some(errors),
										})
									} else {
										Err(Error::Approval {
											errors: Some(errors),
										})
									}
								}
							}
							Err(e) => Err(Error::ProcessFile {
								source: Box::new(e),
							}),
						}
					}
				};

				match is_allowed {
					Ok(_) => Ok(match min_approvals_required {
						Some(min_approvals_required) => {
							let has_bot_approved =
								approved_reviews.iter().any(|review| {
									review
										.user
										.as_ref()
										.map(|user| {
											user.type_field
												.as_ref()
												.map(|type_field| {
													*type_field == UserType::Bot
												})
												.unwrap_or(false)
										})
										.unwrap_or(false)
								});
							// If the bot has already approved, then approving again will not make a
							// difference.
							if has_bot_approved {
								Ok(None)
							} else {
								let relevant_approvals = approved_reviews
									.iter()
									.filter(|review| {
										review
											.user
											.as_ref()
											.map(|user| {
												user.type_field
													.as_ref()
													.map(|type_field| {
														*type_field
															== UserType::User
													})
													.unwrap_or(false)
											})
											.unwrap_or(false)
									})
									.count();
								// See if the target approval count can be reached with the bot's approval
								let bot_approval = 1;
								if relevant_approvals + bot_approval
									== min_approvals_required
								{
									if team_leads.iter().any(|team_lead| {
										team_lead.login == requested_by
									}) {
										Ok(Some("a team lead".to_string()))
									} else {
										process::get_process(
											github_bot, owner, repo_name,
											pr.number,
										)
										.await
										.map(|(process, _)| {
											if process.is_owner(requested_by) {
												Some(
													"a project owner"
														.to_string(),
												)
											} else {
												None
											}
										})
									}
								} else {
									Ok(None)
								}
							}
						}
						_ => Ok(None),
					}),
					Err(e) => Err(e),
				}
			}
			Err(e) => Err(e),
		}
	} else {
		Err(Error::Message {
			msg: format!("{} is not mergeable", pr.html_url),
		})
	}
	.map_err(|e| {
		e.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
	})
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
async fn register_merge_request(
	db: &DB,
	head_sha: &str,
	mr: &MergeRequest,
) -> Result<()> {
	log::info!("Serializing merge request to the database: {:?}", mr);
	let bytes = bincode::serialize(mr).context(Bincode)?;
	db.put(head_sha.as_bytes(), bytes).context(Db)?;
	Ok(())
}

/// Create a merge request, add it to the database, and post a comment stating the merge is
/// pending.
pub async fn wait_to_merge(
	github_bot: &GithubBot,
	db: &DB,
	head_sha: &str,
	mr: MergeRequest,
) -> Result<()> {
	let MergeRequest {
		html_url,
		contributor,
		contributor_repo,
		number,
		..
	} = &mr;
	log::info!("{} checks incomplete.", html_url);
	register_merge_request(db, head_sha, &mr).await?;
	log::info!("Waiting for commit status.");
	let _ = github_bot
		.create_issue_comment(
			contributor,
			contributor_repo,
			*number,
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

async fn recover_from_outdated_merge_target(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
	head_sha: &str,
) -> Result<Option<String>> {
	log::info!(
		"Attempting to recover from possibly outdated target branch (from {}/{}/{} into {}/{}/master)",
		contributor,
		contributor_repo,
		contributor_branch,
		owner,
		owner_repo,
	);
	update_repository(
		github_bot,
		owner,
		owner_repo,
		contributor,
		contributor_repo,
		contributor_branch,
		None,
	)
	.await
	.map(|output| {
		if output.head_sha == head_sha {
			None
		} else {
			Some(output.head_sha)
		}
	})
}

/// Send a merge request.
/// It might recursively call itself when attempting to solve a merge error after something
/// meaningful happens.
#[async_recursion]
async fn merge(
	github_bot: &GithubBot,
	bot_config: &BotConfig,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
	head_sha: &str,
	pr: &PullRequest,
	requested_by: &str,
	created_approval_id: Option<i64>,
) -> Result<Result<()>> {
	match github_bot
		.merge_pull_request(contributor, contributor_repo, pr.number, head_sha)
		.await
	{
		Ok(_) => {
			log::info!("{} merged successfully.", pr.html_url);
			Ok(Ok(()))
		}
		Err(e) => match e {
			Error::Response {
				ref status,
				ref body,
			} => match *status {
				StatusCode::METHOD_NOT_ALLOWED => {
					match body.get("message") {
						Some(message) => {
							let msg = message.to_string();
							if let Some(result) = {
								// Matches the following
								// - "Required status check ... is {pending,expected}."
								// - "... required status checks have not succeeded: ... {pending,expected}."
								let missing_status_matcher = RegexBuilder::new(
									r"required\s+status\s+.*(pending|expected)",
								)
								.case_insensitive(true)
								.build()
								.unwrap();

								if missing_status_matcher
									.find(&msg)
									.is_some()
								{
									// This problem will be solved automatically when all the
									// required statuses are delivered, thus it can be ignored here
									log::info!(
										"Ignoring merge failure due to pending required status; msg: {}",
										&msg
									);
									Some(Ok(Err(Error::Skipped {})))
								} else {
									None
								}
							}
							{
								result
							} else if let Some(result) = {
								if created_approval_id.is_some() {
									// Already attempting to recover; guard against infinite
									// recursion
									None
								} else {
									// Matches the following
									// - "At least N approving reviews are required by reviewers with write access."
									let insufficient_approval_quota_matcher =
										RegexBuilder::new(r"([[:digit:]]+).*approving\s+reviews?\s+(is|are)\s+required")
											.case_insensitive(true)
											.build()
											.unwrap();
									if let Some(matches) = insufficient_approval_quota_matcher.captures(&msg.to_string()) {
										let min_approvals_required = matches
											.get(1)
											.unwrap()
											.as_str()
											.parse::<usize>()
											.unwrap();
										Some(match merge_allowed(
											github_bot,
											contributor,
											contributor_repo,
											pr,
											bot_config,
											requested_by,
											Some(min_approvals_required),
										)
										.await
										{
											Ok(result) => match result {
												Ok(requester_role) => match requester_role {
													Some(requester_role) => {
														let _ = github_bot
															.create_issue_comment(
																contributor,
																&contributor_repo,
																pr.number,
																&format!(
																	"Bot will approve on the behalf of @{}, since they are {}, in an attempt to reach the minimum approval count",
																	requested_by,
																	requester_role,
																),
															)
															.await
															.map_err(|e| {
																log::error!("Error posting comment: {}", e);
															});
														match github_bot.approve_merge_request(
															contributor,
															contributor_repo,
															pr.number
														).await {
															Ok(review) => merge(
																github_bot,
																bot_config,
																owner,
																owner_repo,
																contributor,
																contributor_repo,
																contributor_branch,
																&head_sha,
																pr,
																requested_by,
																Some(review.id),
															).await,
															Err(e) => Err(e)
														}
													},
													None => Err(Error::Message {
														msg: "Requester's approval is not enough to make the PR mergeable".to_string()
													}),
												},
												Err(e) => Err(e)
											},
											Err(e) => Err(e),
										}
										.map_err(|e| Error::Message {
											msg: format!(
												"Could not recover from: `{}` due to: `{}`",
												msg,
												e
											)
										}))
									} else {
										None
									}
								}
							}
							{
								result
							} else if (&msg).contains("Pull Request is not mergeable") {
								match recover_from_outdated_merge_target(
									github_bot,
									owner,
									owner_repo,
									contributor,
									contributor_repo,
									contributor_branch,
									head_sha,
								).await {
									Ok(head_sha) => {
										match head_sha {
											Some(head_sha) => merge(
												github_bot,
												bot_config,
												owner,
												owner_repo,
												contributor,
												contributor_repo,
												contributor_branch,
												&head_sha,
												pr,
												requested_by,
												created_approval_id,
											).await,
											None => Err(Error::Message {
												msg: "PR is not mergeable, but this problem can't be solved by merging master into it.".to_string()
											})
										}
									},
									Err(e) => Err(e)
								}
								.map_err(|e| Error::Message {
									msg: format!(
										"Could not recover from: `{}` due to: `{}`",
										msg,
										e
									)
								})
							} else {
								Err(Error::Message { msg })
							}
						}
						_ => Err(Error::Message {
							msg: format!(
								"
While trying to recover from failed HTTP request (status {}):

Pull Request Merge Endpoint responded with unexpected body: `{}`",
								status, body
							),
						}),
					}
				}
				_ => Err(e),
			},
			_ => Err(e),
		}
		.map_err(|e| Error::Merge {
			source: Box::new(e),
			commit_sha: head_sha.to_string(),
			pr_url: pr.url.to_string(),
			contributor: contributor.to_string(),
			contributor_repo: contributor_repo.to_string(),
			contributor_branch: contributor_branch.to_string(),
			pr_number: pr.number,
			created_approval_id
		}),
	}
	.map_err(|e| {
		e.map_issue((contributor.to_string(), contributor_repo.to_string(), pr.number))
	})
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

fn display_errors_along_the_way(errors: Option<Vec<String>>) -> String {
	errors
		.map(|errors| {
			if errors.len() == 0 {
				"".to_string()
			} else {
				format!(
					"The following errors *might* have affected the outcome of this attempt:\n{}",
					errors.iter().map(|e| format!("- {}", e)).join("\n")
				)
			}
		})
		.unwrap_or_else(|| "".to_string())
}

async fn handle_error_inner(err: Error, state: &AppState) -> Option<String> {
	match err {
		Error::Merge { source, commit_sha, pr_url, contributor, contributor_repo, pr_number, created_approval_id, .. } => {
			let _ = state.db.delete(commit_sha.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			let github_bot = &state.github_bot;
			if let Some(created_approval_id) = created_approval_id {
				let _ = github_bot.clear_merge_request_approval(
					&contributor,
					&contributor_repo,
					pr_number,
					created_approval_id
				).await.map_err(
					|e| log::error!("Failed to cleanup a bot review in {} due to: {}", pr_url, e)
				);
			}
			match *source {
				Error::Response {
					body,
					status
				} => Some(format!("Merge failed with response status: {} and body: `{}`", status, body)),
				Error::Http { source, .. } => {
					Some(format!("Merge failed due to network error:\n\n{}", source))
				}
				Error::Message { .. } => {
					Some(format!("Merge failed: {}", *source))
				}
				_ => Some("Merge failed due to unexpected error".to_string()),
			}
		}
		Error::ProcessFile { source } => match *source {
			Error::Response {
				body: serde_json::Value::Object(m),
				..
			} => Some(format!("Error getting {}: `{}`", PROCESS_FILE, m["message"])),
			Error::Http { source, .. } => {
				Some(format!("Network error getting {}:\n\n{}", PROCESS_FILE, source))
			}
			_ => Some(format!("Unexpected error getting {}:\n\n{}", PROCESS_FILE, source)),
		},
		Error::ProcessInfo { errors } => {
			Some(
				format!(
					"Error: Missing process info. Check that the project for this pull request is defined in {} and no process-related errors have been listed below.\n\n{}\n\n{}",
					PROCESS_FILE,
					display_errors_along_the_way(errors),
					TROUBLESHOOT_MSG
				)
			)
		}
		Error::Approval { errors } => {
			Some(
				format!(
					"Error: Approval criteria was not satisfied.\n\n{}\n\n{}",
					display_errors_along_the_way(errors),
					TROUBLESHOOT_MSG
				)
			)
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
	match e {
		Error::Skipped { .. } => (),
		e => match e {
			Error::WithIssue {
				source,
				issue: (owner, repo, number),
				..
			} => match *source {
				Error::Skipped { .. } => (),
				e => {
					log::error!("handle_error: {}", e);
					let msg = handle_error_inner(e, state)
						.await
						.unwrap_or_else(|| {
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
			},
			_ => {
				log::error!("handle_error: {}", e);
				handle_error_inner(e, state).await;
			}
		},
	}
}
