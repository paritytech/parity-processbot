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
	auth::GithubUserAuthenticator, companion::*, constants::*,
	error::MergeError, github::merge_request::merge, github::*,
	github_bot::GithubBot, rebase::*, vanity_service, Result, Status,
};

/// Check the SHA1 signature on a webhook payload.
fn verify(
	secret: &[u8],
	msg: &[u8],
	signature: &[u8],
) -> Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	hmac::verify(&key, msg, signature)
}

async fn handle_payload(payload: Payload, state: &AppState) -> Result<()> {
	match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			comment:
				Comment {
					ref body,
					user:
						Some(User {
							ref login,
							ref type_field,
						}),
					..
				},
			issue,
		} => match type_field {
			Some(UserType::Bot) => Ok(()),
			_ => match &issue {
				WebhookIssueComment {
					number,
					html_url,
					repository_url: Some(repo_url),
					pull_request: Some(_),
				} => handle_comment(
					body, login, *number, html_url, repo_url, state,
				)
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
				}),
				_ => Ok(()),
			},
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

async fn handle_check(
	status: CheckRunStatus,
	commit_sha: String,
	state: &AppState,
) -> Result<()> {
	if status == CheckRunStatus::Completed {
		check_statuses(&state.github_bot, &state.db, &commit_sha).await
	} else {
		Ok(())
	}
}

async fn handle_status(
	commit_sha: String,
	status: StatusState,
	state: &AppState,
) -> Result<()> {
	if status == StatusState::Pending {
		Ok(())
	} else {
		check_statuses(&state.github_bot, &state.db, &commit_sha).await
	}
}

pub async fn handle_webhook(
	req: Request<Body>,
	state: Arc<Mutex<AppState>>,
) -> Result<()> {
	// Lock here so that the webhooks are processed in the same order they were received; effectively
	// the webhooks will be processed serially
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

	let mut msg_bytes = vec![];
	while let Some(item) = req.body_mut().next().await {
		msg_bytes.extend_from_slice(&item.ok().context(Message {
			msg: format!("Error getting bytes from request body"),
		})?);
	}

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

async fn handle_comment(
	body: &str,
	requested_by: &str,
	number: usize,
	html_url: &str,
	repo_url: &str,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;

	let owner = owner_from_html_url(html_url).context(Message {
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

		merge_allowed(github_bot, owner, &repo_name, pr, requested_by, None)
			.await??;

		if is_ready_to_merge(github_bot, owner, &repo_name, pr).await? {
			prepare_to_merge(
				github_bot,
				owner,
				&repo_name,
				pr.number,
				&pr.html_url,
			)
			.await?;

			match merge(github_bot, owner, &repo_name, pr, requested_by, None)
				.await?
			{
				// If the merge failure will be solved later, then register the PR in the database so that
				// it'll eventually resume processing when later statuses arrive
				Err(MergeError::FailureWillBeSolvedLater) => {
					let _ = register_merge_request(
						owner,
						&repo_name,
						pr.number,
						&pr.html_url,
						requested_by,
						&pr.head_sha()?,
						db,
					);
					return Err(Error::Skipped);
				}
				Err(MergeError::Error(e)) => return Err(e),
				_ => (),
			}

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

		merge_allowed(github_bot, owner, &repo_name, &pr, requested_by, None)
			.await??;

		prepare_to_merge(
			github_bot,
			owner,
			&repo_name,
			pr.number,
			&pr.html_url,
		)
		.await?;

		match merge(github_bot, owner, &repo_name, &pr, requested_by, None)
			.await?
		{
			// Even if the merge failure can be solved later, it does not matter because `merge force` is
			// supposed to be immediate. We should give up here and yield the error message.
			Err(Error::MergeFailureWillBeSolvedLater { msg }) => {
				return Err(Error::MergeAttemptFailed {
					source: Box::new(Error::Message { msg }),
					commit_sha: pr.head_sha()?.to_owned(),
					owner: owner.to_string(),
					repo: repo_name.to_string(),
					pr_number: pr.number,
					created_approval_id: None,
				}
				.map_issue((
					owner.to_string(),
					repo_name.to_string(),
					pr.number,
				)))
			}
			Err(e) => return Err(e),
			_ => (),
		}
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
		}?;
	}

	Ok(())
}
