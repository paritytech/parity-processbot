use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use hyper::{Body, Request, Response, StatusCode};
use ring::hmac;
use snafu::{OptionExt, ResultExt};
use tokio::{sync::Mutex, time::delay_for};

use crate::{
	core::{
		handle_command, process_commit_checks_and_statuses, AppState,
		CommentCommand, MergeCommentCommand, PullRequestMergeCancelOutcome,
	},
	error::{self, handle_error, Error, PullRequestDetails},
	github::*,
	merge_request::{
		cleanup_merge_request, MergeRequest, MergeRequestCleanupReason,
	},
	types::Result,
	WEBHOOK_PARSING_ERROR_TEMPLATE,
};

fn verify_github_webhook_signature(
	secret: &[u8],
	msg: &[u8],
	signature: &[u8],
) -> Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	hmac::verify(&key, msg, signature)
}

pub async fn handle_http_request_for_bot(
	req: Request<Body>,
	state: Arc<Mutex<AppState>>,
) -> Result<Response<Body>> {
	if req.uri().path() == "/webhook" {
		let state = &*state.lock().await;

		if let Some((merge_cancel_outcome, err)) =
			match process_webhook_request(req, state).await {
				Ok((merge_cancel_outcome, result)) => match result {
					Ok(_) => None,
					Err(err) => Some((merge_cancel_outcome, err)),
				},
				Err(err) => {
					Some((PullRequestMergeCancelOutcome::WasNotCancelled, err))
				}
			} {
			handle_error(merge_cancel_outcome, err, state).await
		};

		Response::builder()
			.status(StatusCode::OK)
			.body(Body::from(""))
			.ok()
			.context(error::Message {
				msg: "Error building response".to_owned(),
			})
	} else if req.uri().path() == "/health" {
		Response::builder()
			.status(StatusCode::OK)
			.body(Body::from("OK"))
			.ok()
			.context(error::Message {
				msg: "Healthcheck".to_owned(),
			})
	} else {
		Response::builder()
			.status(StatusCode::NOT_FOUND)
			.body(Body::from("Not found."))
			.ok()
			.context(error::Message {
				msg: "Error building response".to_owned(),
			})
	}
}

pub async fn process_webhook_request(
	mut req: Request<Body>,
	state: &AppState,
) -> Result<(PullRequestMergeCancelOutcome, Result<()>)> {
	let mut msg_bytes = vec![];
	while let Some(item) = req.body_mut().next().await {
		msg_bytes.extend_from_slice(&item.ok().context(error::Message {
			msg: "Error getting bytes from request body".to_owned(),
		})?);
	}

	let webhook_signature = req
		.headers()
		.get("x-hub-signature")
		.context(error::Message {
			msg: "Missing x-hub-signature".to_string(),
		})?
		.to_str()
		.ok()
		.context(error::Message {
			msg: "Error parsing x-hub-signature".to_owned(),
		})?
		.replace("sha1=", "");
	let sig_bytes = base16::decode(webhook_signature.as_bytes()).ok().context(
		error::Message {
			msg: "Error decoding x-hub-signature".to_owned(),
		},
	)?;

	let AppState { config, .. } = state;

	verify_github_webhook_signature(
		config.webhook_secret.trim().as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.ok()
	.context(error::Message {
		msg: "Validation signature does not match".to_owned(),
	})?;

	log::info!("Parsing payload {}", String::from_utf8_lossy(&msg_bytes));
	match serde_json::from_slice::<GithubWebhookPayload>(&msg_bytes) {
		Ok(payload) => Ok(handle_github_payload(payload, state).await),
		Err(err) => {
			// If this comment was originated from a Bot, then acting on it might make the bot
			// to respond to itself recursively, as happened on
			// https://github.com/paritytech/substrate/pull/8409. Therefore we'll only act on
			// this error if it's known for sure it has been initiated only by a User comment.
			let pr_details = serde_json::from_slice::<
				DetectUserCommentPullRequest,
			>(&msg_bytes)
			.ok()
			.and_then(|detected| detected.get_issue_details());

			if let Some(pr_details) = pr_details {
				Err(Error::Message {
					msg: format!(
						WEBHOOK_PARSING_ERROR_TEMPLATE!(),
						err,
						String::from_utf8_lossy(&msg_bytes)
					),
				}
				.with_pr_details(pr_details))
			} else {
				log::info!("Ignoring payload parsing error",);
				Ok((PullRequestMergeCancelOutcome::ShaNotFound, Ok(())))
			}
		}
	}
}

pub async fn handle_github_payload(
	payload: GithubWebhookPayload,
	state: &AppState,
) -> (PullRequestMergeCancelOutcome, Result<()>) {
	let (result, sha) = match payload {
		GithubWebhookPayload::IssueComment {
			action: GithubIssueCommentAction::Unknown,
			..
		} => (Ok(()), None),
		GithubWebhookPayload::IssueComment {
			action: GithubIssueCommentAction::Created,
			comment,
			issue,
		} => match comment {
			GithubComment {
				ref body,
				user:
					Some(GithubUser {
						ref login,
						ref type_field,
					}),
				..
			} => match type_field {
				Some(GithubUserType::Bot) => (Ok(()), None),
				_ => match &issue {
					GithubWebhookIssueComment {
						number,
						html_url,
						repository_url,
						pull_request: Some(_),
					} => {
						let (sha, result) = handle_pull_request_comment(
							body,
							login,
							*number,
							html_url,
							repository_url,
							state,
						)
						.await;
						(
							result.map_err(|err| match err {
								Error::WithPullRequestDetails { .. } => err,
								err => {
									if let Some(details) =
										issue.get_issue_details()
									{
										err.with_pr_details(details)
									} else {
										err
									}
								}
							}),
							sha,
						)
					}
					_ => (Ok(()), None),
				},
			},
			_ => (Ok(()), None),
		},
		GithubWebhookPayload::CommitStatus { sha, state: status } => (
			match status {
				GithubCommitStatusState::Unknown => Ok(()),
				_ => process_commit_checks_and_statuses(state, &sha).await,
			},
			Some(sha),
		),
		GithubWebhookPayload::CheckRun {
			check_run:
				GithubCheckRun {
					status,
					head_sha: sha,
					..
				},
			..
		} => (
			match status {
				GithubCheckRunStatus::Completed => {
					process_commit_checks_and_statuses(state, &sha).await
				}
				_ => Ok(()),
			},
			Some(sha),
		),
		GithubWebhookPayload::WorkflowJob {
			workflow_job:
				GithubWorkflowJob {
					head_sha: sha,
					conclusion,
				},
			..
		} => (
			if conclusion.is_some() {
				process_commit_checks_and_statuses(state, &sha).await
			} else {
				Ok(())
			},
			Some(sha),
		),
	};

	// From this point onwards we'll clean the SHA from the database if this is a error which stops
	// the merge process

	// Without the SHA we'll not be able to fetch the database for more context, so exit early
	let sha = match sha {
		Some(sha) => sha,
		None => return (PullRequestMergeCancelOutcome::ShaNotFound, result),
	};

	// If it's not an error then don't bother with going further
	let err = match result {
		Ok(_) => {
			return (PullRequestMergeCancelOutcome::WasNotCancelled, Ok(()))
		}
		Err(err) => err,
	};

	// If this error does not interrupt the merge process, then don't bother with going further
	if !err.stops_merge_attempt() {
		log::info!(
			"SHA {} did not have its merge attempt stopped because error does not stop the merge attempt {:?}",
			sha,
			err
		);
		return (PullRequestMergeCancelOutcome::WasNotCancelled, Err(err));
	};

	log::info!(
		"SHA {} will have its merge attempt stopped due to {:?}",
		sha,
		err
	);

	match state.db.get(sha.as_bytes()) {
		Ok(Some(bytes)) => {
			match bincode::deserialize::<MergeRequest>(&bytes)
				.context(error::Bincode)
			{
				Ok(mr) => {
					let merge_cancel_outcome = match cleanup_merge_request(
						state,
						&sha,
						&mr.owner,
						&mr.repo,
						mr.number,
						&MergeRequestCleanupReason::Cancelled,
					)
					.await
					{
						Ok(_) => {
							log::info!(
								"Merge of {} (sha {}) was cancelled due to {:?}",
								&mr.html_url,
								sha,
								err
							);
							PullRequestMergeCancelOutcome::WasCancelled
						}
						Err(err) => {
							log::error!(
									"Failed to cancel merge of {} (sha {}) in handle_payload due to {:?}",
									&mr.html_url,
									sha,
									err
								);
							PullRequestMergeCancelOutcome::WasNotCancelled
						}
					};

					(
						merge_cancel_outcome,
						Err(err.with_pr_details(PullRequestDetails {
							owner: mr.owner,
							repo: mr.repo,
							number: mr.number,
						})),
					)
				}
				Err(db_err) => {
					log::error!(
						"Failed to parse {} from the database due to {:?}",
						&sha,
						db_err
					);
					(PullRequestMergeCancelOutcome::WasNotCancelled, Err(err))
				}
			}
		}
		Ok(None) => (PullRequestMergeCancelOutcome::ShaNotFound, Err(err)),
		Err(db_err) => {
			log::info!(
				"Failed to fetch {} from the database due to {:?}",
				sha,
				db_err
			);
			(PullRequestMergeCancelOutcome::WasNotCancelled, Err(err))
		}
	}
}

/// Parse bot commands in pull request comments.
/// The first member of the returned tuple is the relevant commit SHA to invalidate from the
/// database in case of errors.
/// The second member of the returned tuple is the result of handling the parsed command.
async fn handle_pull_request_comment(
	body: &str,
	requested_by: &str,
	number: i64,
	html_url: &str,
	repo_url: &str,
	state: &AppState,
) -> (Option<String>, Result<()>) {
	let cmd = match parse_bot_comment_from_text(body) {
		Some(cmd) => cmd,
		None => return (None, Ok(())),
	};
	log::info!("{:?} requested by {} in {}", cmd, requested_by, html_url);

	let AppState {
		gh_client, config, ..
	} = state;

	let (owner, repo, pr) = match async {
		let owner = owner_from_html_url(html_url).context(error::Message {
			msg: format!("Failed parsing owner in url: {}", html_url),
		})?;

		let repo = repo_url.rsplit('/').next().context(error::Message {
			msg: format!("Failed parsing repo name in url: {}", repo_url),
		})?;

		if !config.disable_org_check {
			gh_client.org_member(owner, requested_by).await?;
		}

		if let CommentCommand::Merge(_) = cmd {
			// We've noticed the bot failing for no human-discernable reason when, for instance, it
			// complained that the pull request was not mergeable when, in fact, it seemed to be, if one
			// were to guess what the state of the Github API was at the time the response was received with
			// "second" precision. For the lack of insight onto the Github Servers, it's assumed that those
			// failures happened because the Github API did not update fast enough and therefore the state
			// was invalid when the request happened, but it got cleared shortly after (possibly
			// microseconds after, hence why it is not discernable at "second" resolution).
			// As a workaround we'll wait for long enough so that Github hopefully has time to update the
			// API and make our merges succeed. A proper workaround would also entail retrying every X
			// seconds for recoverable errors such as "required statuses are missing or pending".
			delay_for(Duration::from_millis(config.merge_command_delay)).await;
		};

		let pr = gh_client.pull_request(owner, repo, number).await?;

		Ok((owner, repo, pr))
	}
	.await
	{
		Ok(value) => value,
		Err(err) => return (None, Err(err)),
	};

	let result = handle_command(state, &cmd, &pr, requested_by)
		.await
		.map_err(|err| {
			err.with_pr_details(PullRequestDetails {
				owner: owner.into(),
				repo: repo.into(),
				number,
			})
		});

	let sha = match cmd {
		CommentCommand::Merge(_) => Some(pr.head.sha),
		_ => None,
	};

	(sha, result)
}

pub fn parse_bot_comment_from_text(text: &str) -> Option<CommentCommand> {
	let text = text.to_lowercase();
	let text = text.trim();

	let cmd = match text {
		"bot merge" => CommentCommand::Merge(MergeCommentCommand::Normal),
		"bot merge force" => CommentCommand::Merge(MergeCommentCommand::Force),
		"bot merge cancel" => CommentCommand::CancelMerge,
		"bot rebase" => CommentCommand::Rebase,
		_ => return None,
	};

	Some(cmd)
}
