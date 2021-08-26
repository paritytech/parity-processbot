use async_recursion::async_recursion;
use futures::StreamExt;
use hyper::{Body, Request, Response, StatusCode};
use regex::RegexBuilder;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use crate::{constants::*, error::*, github::*, types::*};

async fn process_checks(
	status: CheckRunStatus,
	state: &AppState,
	commit_sha: String,
) -> Result<()> {
	if status == CheckRunStatus::Completed {
		return state.check_statuses(commit_sha).await;
	}

	Ok(())
}

struct ProcessCommentArgs<'a> {
	body: &'a str,
	html_url: &'a str,
	repo_url: &'a str,
	requested_by: &'a str,
	number: &'a usize,
}
async fn process_comment<'a>(
	args: ProcessCommentArgs<'a>,
	state: &AppState,
) -> Result<()> {
	let ProcessCommentArgs {
		body,
		html_url,
		repo_url,
		requested_by,
		number,
	} = args;
	let AppState { db, github_bot, .. } = state;

	let owner =
		parse_owner_from_html_url(html_url).context(Error::Message {
			msg: format!("Failed parsing owner in url: {}", html_url),
		})?;

	let repo_name = repo_url.rsplit('/').next().context(Message {
		msg: format!("Failed parsing repo name in url: {}", repo_url),
	})?;

	// Fetch the pr to get all fields (eg. mergeable).
	let pr = github_bot
		.pull_request(PullRequestArgs {
			owner,
			repo_name,
			number,
		})
		.await?;

	let auth =
		GithubUserAuthenticator::new(requested_by, owner, &repo_name, number);

	if body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim() {
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		auth.check_org_membership(&github_bot).await?;

		github_bot
			.merge_allowed(MergeAllowedArgs {
				owner,
				repo_name,
				pr,
				requested_by,
				min_approvals_required: None,
			})
			.await??;

		if is_ready_to_merge(github_bot, owner, &repo_name, pr).await? {
			github_bot
				.prepare_to_merge(PrepareToMergeArgs {
					owner,
					repo_name,
					number: pr.number,
					html_url: &pr.html_url,
				})
				.await?;

			match github_bot
				.merge(MergeArgs {
					owner,
					repo_name,
					pr,
					requested_by,
					created_approval_id: None,
				})
				.await?
			{
				Ok(_) => (),
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

		github_bot
			.merge_allowed(MergeAllowedArgs {
				owner,
				repo_name,
				pr,
				requested_by,
				min_approvals_required: None,
			})
			.await??;

		github_bot
			.prepare_to_merge(
				github_bot,
				PrepareToMergeArgs {
					owner,
					repo_name,
					number: pr.number,
					html_url: &pr.html_url,
				},
			)
			.await?;

		match merge(github_bot, owner, &repo_name, &pr, requested_by, None)
			.await?
		{
			Ok(_) => Ok(()),
			// Even if the merge failure can be solved later, it does not matter because `merge force` is
			// supposed to be immediate. We should give up here and yield the error message.
			Err(MergeError::FailureWillBeSolvedLater) => {
				Err(Error::MergeAttemptFailed {
					source: Box::new(Error::Message {
						msg: "Pull request is not mergeable",
					}),
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
			Err(MergeError::Error(e)) => Err(e),
		}?;

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
		github_bot
			.create_issue_comment(CreateIssueCommentArgs {
				owner,
				repo_name: &repo_name,
				number: pr.number,
				body: "Merge cancelled.",
			})
			.await?;
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
					.create_issue_comment(CreateIssueCommentArgs {
						owner,
						repo_name,
						number: pr.number,
						body: "Rebasing.",
					})
					.await?;
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

async fn process_status(
	commit_sha: String,
	status: StatusState,
	state: &AppState,
) -> Result<()> {
	if status == StatusState::Pending {
		return Ok(());
	}

	check_statuses(&state.github_bot, &state.db, &commit_sha).await
}

async fn process_payload(payload: Payload, state: &AppState) -> Result<()> {
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
							..
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
				} => process_comment(
					ProcessCommentArgs {
						body,
						number,
						html_url,
						repo_url,
					},
					state,
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
			process_status(sha, status, state).await
		}
		Payload::CheckRun {
			check_run: CheckRun {
				status, head_sha, ..
			},
			..
		} => process_checks(status, state, head_sha).await,
		_ => Ok(()),
	}
}

pub async fn handle_request(
	req: Request<Body>,
	state: Arc<Mutex<AppState>>,
) -> Result<()> {
	// Lock here so that the webhooks are processed in the same order they were received; effectively
	// the webhooks will be processed serially
	let state = &*state.lock().await;

	let sig = req
		.headers()
		.get("x-hub-signature")
		.context(Error::Message {
			msg: format!("Missing x-hub-signature"),
		})?
		.to_str()
		.ok()
		.context(Error::Message {
			msg: format!("Error parsing x-hub-signature"),
		})?
		.to_string();
	log::info!("Lock acquired for {:?}", sig);

	let mut msg_bytes = vec![];
	while let Some(item) = req.body_mut().next().await {
		msg_bytes.extend_from_slice(&item.ok().context(Error::Message {
			msg: format!("Error getting bytes from request body"),
		})?);
	}

	let key = hmac::Key::new(
		hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
		state.webhook_secret.trim().as_bytes(),
	);
	hmac::verify(&key, &msg_bytes, &sig_bytes).ok().context(
		Error::Message {
			msg: format!("Validation signature does not match"),
		},
	)?;

	log::info!("Parsing payload {}", String::from_utf8_lossy(&msg_bytes));
	match serde_json::from_slice::<Payload>(&msg_bytes) {
		Ok(payload) => process_payload(payload, state).await,
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
