use async_recursion::async_recursion;
use futures::StreamExt;
use html_escape;
use hyper::{Body, Request, Response, StatusCode};
use itertools::Itertools;
use regex::RegexBuilder;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::delay_for};

use crate::{
	companion::parse_all_companions, companion::*, config::MainConfig,
	constants::*, error::*, github::*, github_bot::GithubBot, process,
	rebase::*, utils::parse_bot_comment_from_text, vanity_service,
	CommentCommand, MergeCancelOutcome, MergeCommentCommand, Result, Status,
	PROCESS_INFO_ERROR_TEMPLATE, WEBHOOK_PARSING_ERROR_TEMPLATE,
};

/// This data gets passed along with each webhook to the webhook handler.
pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub config: MainConfig,
}

/// This stores information about a pull request while we wait for checks to complete.
#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequestBase {
	pub owner: String,
	pub repo: String,
	pub number: i64,
}
#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	pub owner: String,
	pub repo: String,
	pub number: i64,
	pub html_url: String,
	pub requested_by: String,
	pub companion_children: Option<Vec<MergeRequestBase>>,
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
		if let Some((merge_cancel_outcome, err)) =
			match webhook_inner(req, state).await {
				Ok((merge_cancel_outcome, result)) => match result {
					Ok(_) => None,
					Err(err) => Some((merge_cancel_outcome, err)),
				},
				Err(err) => Some((MergeCancelOutcome::WasNotCancelled, err)),
			} {
			handle_error(merge_cancel_outcome, err, state).await
		};
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
) -> Result<(MergeCancelOutcome, Result<()>)> {
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

	let AppState { config, .. } = state;

	verify(
		config.webhook_secret.trim().as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.ok()
	.context(Message {
		msg: format!("Validation signature does not match"),
	})?;

	log::info!("Parsing payload {}", String::from_utf8_lossy(&msg_bytes));
	match serde_json::from_slice::<Payload>(&msg_bytes) {
		Ok(payload) => Ok(handle_payload(payload, state).await),
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
						WEBHOOK_PARSING_ERROR_TEMPLATE!(),
						err.to_string(),
						String::from_utf8_lossy(&msg_bytes)
					),
				}
				.map_issue(pr_details))
			} else {
				log::info!("Ignoring payload parsing error",);
				Ok((MergeCancelOutcome::ShaNotFound, Ok(())))
			}
		}
	}
}

pub async fn handle_payload(
	payload: Payload,
	state: &AppState,
) -> (MergeCancelOutcome, Result<()>) {
	let (result, sha) = match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Unknown,
			..
		} => (Ok(()), None),
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			comment,
			issue,
		} => match comment {
			Comment {
				ref body,
				user: Some(User {
					ref login,
					ref type_field,
				}),
				..
			} => match type_field {
				Some(UserType::Bot) => (Ok(()), None),
				_ => match &issue {
					WebhookIssueComment {
						number,
						html_url,
						repository_url: Some(repo_url),
						pull_request: Some(_),
					} => {
						let (sha, result) = handle_comment(
							body, login, *number, html_url, repo_url, state,
						)
						.await;
						(
							result.map_err(|err| match err {
								Error::WithIssue { .. } => err,
								err => {
									if let Some(details) =
										issue.get_issue_details()
									{
										err.map_issue(details)
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
		Payload::CommitStatus { sha, state: status } => (
			match status {
				StatusState::Pending => Ok(()),
				_ => checks_and_status(state, &sha).await,
			},
			Some(sha),
		),
		Payload::CheckRun {
			check_run: CheckRun {
				status,
				head_sha: sha,
				..
			},
			..
		} => (
			match status {
				CheckRunStatus::Completed => {
					checks_and_status(state, &sha).await
				}
				_ => Ok(()),
			},
			Some(sha),
		),
		Payload::WorkflowJob {
			workflow_job: WorkflowJob {
				head_sha: sha,
				conclusion,
			},
			..
		} => (
			if conclusion.is_some() {
				checks_and_status(state, &sha).await
			} else {
				Ok(())
			},
			Some(sha),
		),
		Payload::CheckSuite {
			check_suite: CheckSuite {
				head_sha: sha,
				conclusion,
			},
			..
		} => (
			if conclusion.is_some() {
				checks_and_status(state, &sha).await
			} else {
				Ok(())
			},
			Some(sha),
		),
	};

	// From this point onwards we'll clean the SHA from the database if this is a fatal error which
	// stops the merge process

	// Without the SHA we'll not be able to fetch the database for more context, so exit early
	let sha = match sha {
		Some(sha) => sha,
		None => return (MergeCancelOutcome::ShaNotFound, result),
	};

	// If it's not an error then don't bother with going further
	let err = match result {
		Ok(_) => return (MergeCancelOutcome::WasCancelled, Ok(())),
		Err(err) => err,
	};

	// If this error does not interrupt the merge process, then don't bother with going further
	if !err.stops_merge_attempt() {
		return (MergeCancelOutcome::WasNotCancelled, Err(err));
	};

	match state.db.get(sha.as_bytes()) {
		Ok(Some(bytes)) => {
			match bincode::deserialize::<MergeRequest>(&bytes).context(Bincode)
			{
				Ok(mr) => {
					let merge_cancel_outcome = match cleanup_pr(
						state, &sha, &mr.owner, &mr.repo, mr.number,
					) {
						Ok(_) => {
							log::info!(
								"Merge of {} (sha {}) was cancelled due to {:?}",
								&mr.html_url,
								sha,
								err
							);
							MergeCancelOutcome::WasCancelled
						}
						Err(err) => {
							log::error!(
									"Failed to cancel merge of {} (sha {}) in handle_payload due to {:?}",
									&mr.html_url,
									sha,
									err
								);
							MergeCancelOutcome::WasNotCancelled
						}
					};

					let err = match err {
						Error::WithIssue { .. } => err,
						err => err.map_issue((mr.owner, mr.repo, mr.number)),
					};

					(merge_cancel_outcome, Err(err))
				}
				Err(db_err) => {
					log::error!(
						"Failed to parse {} from the database due to {:?}",
						&sha,
						db_err
					);
					(MergeCancelOutcome::WasNotCancelled, Err(err))
				}
			}
		}
		Ok(None) => (MergeCancelOutcome::ShaNotFound, Err(err)),
		Err(db_err) => {
			log::info!(
				"Failed to fetch {} from the database due to {:?}",
				sha,
				db_err
			);
			(MergeCancelOutcome::WasNotCancelled, Err(err))
		}
	}
}

pub async fn get_latest_statuses_state(
	github_bot: &GithubBot,
	owner: &str,
	repo: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let status = github_bot.status(owner, repo, commit_sha).await?;
	log::info!("{} statuses: {:?}", html_url, status);

	// Since Github only considers the latest instance of each status, we should abide by the same
	// rule. Each instance is uniquely identified by "context".
	let mut latest_statuses: HashMap<String, (i64, StatusState)> =
		HashMap::new();
	for s in status.statuses {
		if s.description
			.as_ref()
			.map(|description| {
				match serde_json::from_str::<vanity_service::JobInformation>(
					description,
				) {
					Ok(info) => info.build_allow_failure.unwrap_or(false),
					_ => false,
				}
			})
			.unwrap_or(false)
		{
			continue;
		}

		if latest_statuses
			.get(&s.context)
			.map(|(prev_id, _)| prev_id < &(&s).id)
			.unwrap_or(true)
		{
			latest_statuses.insert(s.context, (s.id, s.state));
		}
	}
	log::info!("{} latest_statuses: {:?}", html_url, latest_statuses);

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

pub async fn get_latest_checks_state(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let checks = github_bot
		.check_runs(&owner, &repo_name, commit_sha)
		.await?;
	log::info!("{} checks: {:?}", html_url, checks);

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
	log::info!("{} latest_checks,: {:?}", html_url, latest_checks);

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
async fn checks_and_status(state: &AppState, sha: &str) -> Result<()> {
	let AppState { db, github_bot, .. } = state;

	log::info!("Checking for statuses of {}", sha);

	// If a SHA is in the database, it means `bot merge` has been triggered for it specifically, not
	// indirectly through a companion, and so we do not need to dig into it's parental relationship
	// because they are supposed to be processed as an independent unit of work for the merge
	// process.
	let (requested_by, parent, pr) = match db.get(sha.as_bytes()).context(Db)? {
		Some(bytes) => {
			let mr: MergeRequest =
				bincode::deserialize(&bytes).context(Bincode)?;
			let MergeRequest {
				owner,
				repo,
				number,
				..
			} = &mr;
			let pr = github_bot.pull_request(owner, repo, *number).await?;
			log::info!(
				"Deserialized merge request for {} (sha {}): {:?}",
				pr.html_url,
				sha,
				mr
			);
			(mr.requested_by, None, pr)
		}
		None => match get_match_from_registered_companions(state, sha).await? {
			Some(((sha, parent), pr)) => {
				log::info!(
					"Found parent for {} (sha {}): {:?}",
					pr.html_url,
					sha,
					parent
				);
				(parent.requested_by.to_owned(), Some((sha, parent)), pr)
			}
			None => return Ok(()),
		},
	};

	let mut parent_pr_was_merged = false;

	match async {
		if sha != pr.head.sha {
			return Err(Error::HeadChanged {
				expected: sha.to_string(),
				actual: pr.head.sha.to_owned(),
			});
		}

		if !ready_to_merge(github_bot, &pr).await? {
			return Ok(());
		}

		if let Err(err) =
			check_merge_is_allowed(state, &pr, &requested_by, None).await
		{
			return match err {
				Error::InvalidCompanionStatus { ref value, .. } => {
					match value {
						InvalidCompanionStatusValue::Pending => Ok(()),
						InvalidCompanionStatusValue::Failure => Err(err),
					}
				}
				err => Err(err),
			};
		}

		if let Some((parent_sha, parent)) = parent.as_ref() {
			let parent_pr = github_bot
				.pull_request(&parent.owner, &parent.repo, parent.number)
				.await?;

			// Check if this PR is indeed still a companion of the parent (the parent's description might
			// have been edited since this PR was registered as a companion)
			let is_still_companion = parent_pr
				.body
				.as_ref()
				.map(|body| parse_all_companions(body).iter().any(|(html_url, _, _, _)| {
					html_url == &pr.html_url
				})).unwrap_or(false);

			if is_still_companion {
				if parent_sha != parent_pr.head.sha.as_str() {
					return Err(Error::Message {
						msg: format!(
							"Parent HEAD changed from {} to {}",
							parent_sha, parent_pr.head.sha
						),
					});
				}

				if let Err(err) = match check_merge_is_allowed(
					state,
					&parent_pr,
					&requested_by,
					None,
				)
				.await
				{
					Ok(_) => Ok(()),
					Err(err) => match err {
						Error::InvalidCompanionStatus { ref value, .. } => {
							match value {
								InvalidCompanionStatusValue::Pending => {
									log::info!("In checks_and_status, skipped merging {} as a companion because the parent {} had other companions pending", pr.html_url, parent.html_url);
									return Ok(())
								}
								InvalidCompanionStatusValue::Failure => Err(err),
							}
						}
						err => Err(err),
					},
				} {
					let msg = format!(
						"Failed to merge (parent) {} due to {}",
						parent_pr.html_url, err
					);
					return Err(Error::Message { msg });
				}

				if !ready_to_merge(github_bot, &parent_pr).await? {
					return Ok(());
				}

				// Merge the parent (which should also merge this one, since it is a companion)
				log::info!(
					"Merging {} (parent of {}, which will be merged later as a companion)",
					parent_pr.html_url,
					pr.html_url
				);
				if let Err(err) =
					merge(state, &parent_pr, &requested_by, None).await?
				{
					return match err {
						Error::MergeFailureWillBeSolvedLater { .. } => Ok(()),
						err => Err(Error::Message {
							msg: format!(
								"Failed to merge (parent) {} due to {}",
								parent_pr.html_url, err
							),
						}),
					};
				};
				parent_pr_was_merged = true;

				// Merge the parent (which should also merge this one, since it is a companion)
				log::info!(
					"Merging companions of {} (parent of {}, from where this scenario was triggered)",
					parent_pr.html_url,
					pr.html_url
				);

				let should_confirm_pr_merged = match merge_companions(state, &parent_pr, &requested_by, Some(&pr.html_url)).await {
					// Since the parent's companions have been merged, and this PR is a companion, it should have
					// been merged as well.
					Ok(_) => true,
					Err(err) => match err {
						Error::CompanionsFailedMerge { errors } => {
							let this_pr_error = errors.into_iter().find(|CompanionDetailsWithErrorMessage {
								html_url,
								..
							}| {
								html_url == &pr.html_url
							});
							if let Some(this_pr_error) = this_pr_error {
								return Err(Error::Message {
									msg: format!(
										"Failed to merge {} as a companion of {} due to {}",
										pr.html_url,
										parent_pr.html_url,
										this_pr_error.msg
									)
								});
							} else {
								true
							}
						},
						_ => {
							log::error!(
								"Failed to merge companions of {} due to {:?}",
								parent_pr.html_url,
								err
							);
							false
						}
					}
				};

				// From the parent companion merges being executed above, at this point this pull request will
				// either be merged later or it has already been merged. For sanity's sake we'll confirm those
				// assumptions here.
				if should_confirm_pr_merged {
					let pr = github_bot.pull_request(&pr.base.repo.owner.login, &pr.base.repo.name, pr.number).await?;
					if pr.merged || db.get(&pr.head.sha.as_bytes()).context(Db)?.is_some() {
						if let Err(err) = cleanup_pr(
							state,
							&pr.head.sha,
							&pr.base.repo.owner.login,
							&pr.base.repo.name,
							pr.number
						) {
							log::error!(
								"Failed to cleanup PR {} after it has been merged in merge_companions of checks_and_status due to {}",
								pr.html_url,
								err
							);
						};
						return Ok(());
					} else {
						log::error!(
							"Expected {} to have been merged in merge_companions of {}, but it did not happen. This could be a bug.",
							pr.html_url,
							parent_pr.html_url
						);
					}
				}
			}
		}

		if let Err(err) =
			check_merge_is_allowed(state, &pr, &requested_by, None).await
		{
			return match err {
				Error::InvalidCompanionStatus { ref value, .. } => {
					match value {
						InvalidCompanionStatusValue::Pending => {
							log::info!("In checks_and_status, skipped merging {} because it had companions pending", pr.html_url);
							Ok(())
						},
						InvalidCompanionStatusValue::Failure => Err(err),
					}
				}
				err => Err(err),
			};
		};

		if let Err(err) = merge(state, &pr, &requested_by, None).await? {
			return match err {
				Error::MergeFailureWillBeSolvedLater { .. } => Ok(()),
				err => Err(err),
			};
		}

		if let Err(err) = merge_companions(state, &pr, &requested_by, None).await {
			log::error!(
				"Failed to merge companions of {} due to {}",
				pr.html_url,
				err
			);
		}

		Ok(())
	}
	.await
	{
		Ok(_) | Err(Error::MergeFailureWillBeSolvedLater { .. }) => Ok(()),
		Err(err) => {
			// There's no point in cancelling the merge of the parent and communicating and error there
			// if it already has been merged
			if !parent_pr_was_merged {
				if let Some((parent_sha, parent)) = parent {
					cleanup_pr(
						state,
						&parent_sha,
						&parent.owner,
						&parent.repo,
						parent.number,
					)?;
					handle_error(
						MergeCancelOutcome::WasCancelled,
						Error::Message {
							msg: format!(
								"Companion {} has error: {}",
								pr.html_url, err
							),
						}
						.map_issue((parent.owner, parent.repo, parent.number)),
						state,
					)
					.await;
				}
			}
			Err(err.map_issue((
				pr.base.repo.owner.login,
				pr.base.repo.name,
				pr.number,
			)))
		}
	}
}

async fn handle_command(
	state: &AppState,
	cmd: &CommentCommand,
	pr: &PullRequest,
	requested_by: &str,
) -> Result<()> {
	let AppState { github_bot, .. } = state;

	match cmd {
		CommentCommand::Merge(cmd) => {
			let mr = MergeRequest {
				owner: pr.base.repo.owner.login.to_owned(),
				repo: pr.base.repo.name.to_owned(),
				number: pr.number,
				html_url: pr.html_url.to_owned(),
				requested_by: requested_by.to_owned(),
				companion_children: pr.body.as_ref().map(|body| {
					parse_all_companions(body)
						.into_iter()
						.map(|(_, owner, repo, number)| MergeRequestBase {
							owner,
							repo,
							number,
						})
						.collect()
				}),
			};

			let should_wait_for_companions =
				match check_merge_is_allowed(state, &pr, requested_by, None)
					.await
				{
					Ok(_) => false,
					Err(err) => match err {
						Error::InvalidCompanionStatus { ref value, .. } => {
							match value {
								InvalidCompanionStatusValue::Pending => {
									match cmd {
										MergeCommentCommand::Normal => true,
										MergeCommentCommand::Force => false,
									}
								}
								InvalidCompanionStatusValue::Failure => {
									match cmd {
										MergeCommentCommand::Normal => {
											return Err(err)
										}
										MergeCommentCommand::Force => false,
									}
								}
							}
						}
						_ => return Err(err),
					},
				};

			match cmd {
				MergeCommentCommand::Normal => {
					if !should_wait_for_companions
						&& ready_to_merge(github_bot, &pr).await?
					{
						match merge(state, pr, requested_by, None).await? {
							// If the merge failure will be solved later, then register the PR in the database so that
							// it'll eventually resume processing when later statuses arrive
							Err(Error::MergeFailureWillBeSolvedLater {
								msg,
							}) => {
								let msg = format!(
									"This PR cannot be merged **at the moment** due to: {}\n\nprocessbot expects that the problem will be solved automatically later and so the auto-merge process will be started. You can simply wait for now.\n\n",
									msg
								);
								wait_to_merge(
									state,
									&pr.head.sha,
									&mr,
									Some(&msg),
								)
								.await?;
								return Err(
									Error::MergeFailureWillBeSolvedLater {
										msg,
									},
								);
							}
							Err(e) => return Err(e),
							_ => (),
						}
					} else {
						wait_to_merge(
							state,
							&pr.head.sha,
							&mr,
							if should_wait_for_companions {
								Some("Waiting for companions' statuses and this PR's statuses")
							} else {
								None
							},
						)
						.await?;
						return Ok(());
					}
				}
				MergeCommentCommand::Force => {
					match merge(state, pr, requested_by, None).await? {
						// Even if the merge failure can be solved later, it does not matter because `merge force` is
						// supposed to be immediate. We should give up here and yield the error message.
						Err(Error::MergeFailureWillBeSolvedLater { msg }) => {
							return Err(Error::Message { msg })
						}
						Err(e) => return Err(e),
						_ => (),
					}
				}
			}

			log::info!("Merging companions of {}", pr.html_url);
			if let Err(err) =
				merge_companions(state, pr, requested_by, None).await
			{
				log::error!(
					"Failed to merge the companions of {} due to {:?}",
					pr.html_url,
					err
				);
			}

			Ok(())
		}
		CommentCommand::CancelMerge => {
			log::info!("Deleting merge request for {}", pr.html_url);

			cleanup_pr(
				state,
				&pr.head.sha,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
			)?;

			if let Err(err) = github_bot
				.create_issue_comment(
					&pr.base.repo.owner.login,
					&pr.base.repo.name,
					pr.number,
					"Merge cancelled.",
				)
				.await
			{
				log::error!(
					"Failed to post comment on {} due to {}",
					pr.html_url,
					err
				);
			}

			Ok(())
		}
		CommentCommand::Rebase => {
			if let Err(err) = github_bot
				.create_issue_comment(
					&pr.base.repo.owner.login,
					&pr.base.repo.name,
					pr.number,
					"Rebasing",
				)
				.await
			{
				log::error!(
					"Failed to post comment on {} due to {}",
					pr.html_url,
					err
				);
			}

			rebase(
				github_bot,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				&pr.head.repo.owner.login,
				&pr.head.repo.name,
				&pr.head.ref_field,
			)
			.await
		}
		CommentCommand::CompareReleaseRequest => {
			match pr.base.repo.name.as_str() {
				"polkadot" => {
					let rel = github_bot
						.latest_release(
							&pr.base.repo.owner.login,
							&pr.base.repo.name,
						)
						.await?;
					let release_tag = github_bot
						.tag(
							&pr.base.repo.owner.login,
							&pr.base.repo.name,
							&rel.tag_name,
						)
						.await?;
					let release_substrate_commit = github_bot
						.substrate_commit_from_polkadot_commit(
							&release_tag.object.sha,
						)
						.await?;
					let branch_substrate_commit = github_bot
						.substrate_commit_from_polkadot_commit(&pr.head.sha)
						.await?;
					let link = github_bot.diff_url(
						&pr.base.repo.owner.login,
						"substrate",
						&release_substrate_commit,
						&branch_substrate_commit,
					);
					log::info!("Posting link to substrate diff: {}", &link);
					if let Err(err) = github_bot
						.create_issue_comment(
							&pr.base.repo.owner.login,
							&pr.base.repo.name,
							pr.number,
							&link,
						)
						.await
					{
						log::error!(
							"Failed to post comment on {} due to {}",
							pr.html_url,
							err
						);
					}
					Ok(())
				}
				_ => Err(Error::Message {
					msg: "This command can't be requested from this repository"
						.to_string(),
				}),
			}
		}
	}
}

/// Parse bot commands in pull request comments. Commands are listed in README.md.
/// The first member of the returned tuple is the relevant commit SHA to invalidate from the
/// database in case of errors.
/// The second member of the returned tuple is the result of handling the parsed command.
async fn handle_comment(
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
		github_bot, config, ..
	} = state;

	let (owner, repo, pr) = match async {
		let owner =
			GithubBot::owner_from_html_url(html_url).context(Message {
				msg: format!("Failed parsing owner in url: {}", html_url),
			})?;

		let repo = repo_url.rsplit('/').next().context(Message {
			msg: format!("Failed parsing repo name in url: {}", repo_url),
		})?;

		if !config.disable_org_check {
			github_bot.org_member(owner, requested_by).await?;
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
			delay_for(Duration::from_millis(4096)).await;
		};

		let pr = github_bot.pull_request(owner, &repo, number).await?;

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
			err.map_issue((owner.to_owned(), repo.to_owned(), number))
		});

	let sha = match cmd {
		CommentCommand::Merge(_) => Some(pr.head.sha),
		_ => None,
	};

	(sha, result)
}

/// Check if the pull request is mergeable and approved.
/// Errors related to core-devs and substrateteamleads API requests are ignored
/// because the merge might succeed regardless of them, thus it does not make
/// sense to fail this scenario completely if the request fails for some reason.
pub async fn check_merge_is_allowed(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
	min_approvals_required: Option<usize>,
) -> Result<Option<String>> {
	let AppState {
		github_bot, config, ..
	} = state;

	if let Some(min_approvals_required) = &min_approvals_required {
		log::info!(
			"Attempting to reach minimum number of approvals {}",
			min_approvals_required
		);
	} else if pr.mergeable.unwrap_or(false) {
		log::info!("{} is mergeable", pr.html_url);
	} else {
		log::info!("{} is not mergeable", pr.html_url);
	}

	if !pr.mergeable.unwrap_or(false) && min_approvals_required.is_none() {
		return Err(Error::Message {
			msg: format!("Github API says {} is not mergeable", pr.html_url),
		});
	}

	check_all_companions_are_mergeable(github_bot, &pr).await?;

	// Consider only the latest relevant review submitted per user
	let latest_reviews = {
		let reviews = github_bot.reviews(&pr.url).await?;
		let mut latest_reviews = HashMap::new();
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
						latest_reviews.insert(user_login, (review.id, review));
					}
				}
			}
		}
		latest_reviews
	};

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

	let mut errors: Vec<String> = Vec::new();

	let team_leads = if config.disable_org_check {
		vec![]
	} else {
		github_bot
			.substrate_team_leads(&pr.base.repo.owner.login)
			.await
			.unwrap_or_else(|e| {
				let msg = format!(
					"Error getting {}: `{}`",
					SUBSTRATE_TEAM_LEADS_GROUP, e
				);
				log::error!("{}", msg);
				errors.push(msg);
				vec![]
			})
	};
	let team_lead_approvals = approved_reviews
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
		.count();

	let core_devs = if config.disable_org_check {
		vec![]
	} else {
		github_bot
			.core_devs(&pr.base.repo.owner.login)
			.await
			.unwrap_or_else(|e| {
				let msg = format!("Error getting {}: `{}`", CORE_DEVS_GROUP, e);
				log::error!("{}", msg);
				errors.push(msg);
				vec![]
			})
	};
	let core_dev_approvals = approved_reviews
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
		.count();

	let relevant_approvals_count = if core_dev_approvals > team_lead_approvals {
		core_dev_approvals
	} else {
		team_lead_approvals
	};

	let relevant_approvals_count =
		if team_leads.iter().any(|lead| lead.login == requested_by) {
			log::info!("{} merge requested by a team lead.", pr.html_url);
			Ok(relevant_approvals_count)
		} else {
			// TODO have this be based on the repository's settings from the API
			// (https://github.com/paritytech/parity-processbot/issues/319)
			let min_reviewers = match pr.base.repo.name.as_str() {
				"substrate" => 2,
				"polkadot" => 1,
				_ => 0,
			};

			let core_approved = core_dev_approvals >= min_reviewers;
			let lead_approved = team_lead_approvals >= 1;

			if core_approved || lead_approved {
				log::info!("{} has core or team lead approval.", pr.html_url);
				Ok(relevant_approvals_count)
			} else {
				let (process, process_warnings) = process::get_process(
					github_bot,
					&pr.base.repo.owner.login,
					&pr.base.repo.name,
					pr.number,
				)
				.await?;

				let project_owner_approved =
					approved_reviews.iter().rev().any(|review| {
						review
							.user
							.as_ref()
							.map(|user| process.is_owner(&user.login))
							.unwrap_or(false)
					});
				let project_owner_requested = process.is_owner(requested_by);

				if project_owner_approved || project_owner_requested {
					log::info!("{} has project owner approval.", pr.html_url);
					Ok(relevant_approvals_count)
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
		}?;

	let min_approvals_required = match min_approvals_required {
		Some(min_approvals_required) => min_approvals_required,
		None => return Ok(None),
	};

	let has_bot_approved = approved_reviews.iter().any(|review| {
		review
			.user
			.as_ref()
			.map(|user| {
				user.type_field
					.as_ref()
					.map(|type_field| *type_field == UserType::Bot)
					.unwrap_or(false)
			})
			.unwrap_or(false)
	});

	// If the bot has already approved, then approving again will not make a
	// difference.
	if has_bot_approved
	// If the bot's approval is not enough to reach the minimum, then don't bother with approving
		|| relevant_approvals_count + 1 != min_approvals_required
	{
		return Ok(None);
	}

	let role = if team_leads
		.iter()
		.any(|team_lead| team_lead.login == requested_by)
	{
		Some("a team lead".to_string())
	} else {
		let (process, _) = process::get_process(
			github_bot,
			&pr.base.repo.owner.login,
			&pr.base.repo.name,
			pr.number,
		)
		.await?;
		if process.is_owner(requested_by) {
			Some("a project owner".to_string())
		} else {
			None
		}
	};

	Ok(role)
}

/// Query checks and statuses.
///
/// This function is used when a merge request is first received, to decide whether to store the
/// request and wait for checks -- if so they will later be handled by `checks_and_status`.
pub async fn ready_to_merge(
	github_bot: &GithubBot,
	pr: &PullRequest,
) -> Result<bool> {
	match get_latest_statuses_state(
		github_bot,
		&pr.base.repo.owner.login,
		&pr.base.repo.name,
		&pr.head.sha,
		&pr.html_url,
	)
	.await?
	{
		Status::Success => {
			match get_latest_checks_state(
				github_bot,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				&pr.head.sha,
				&pr.html_url,
			)
			.await?
			{
				Status::Success => Ok(true),
				Status::Failure => Err(Error::ChecksFailed {
					commit_sha: pr.head.sha.to_owned(),
				}),
				_ => Ok(false),
			}
		}
		Status::Failure => Err(Error::ChecksFailed {
			commit_sha: pr.head.sha.to_owned(),
		}),
		_ => Ok(false),
	}
}

/// Create a merge request object.
///
/// If this has been called, error handling must remove the db entry.
async fn register_merge_request(
	state: &AppState,
	sha: &str,
	mr: &MergeRequest,
) -> Result<()> {
	let AppState { db, .. } = state;
	log::info!("Registering merge request (sha: {}): {:?}", sha, mr);
	let bytes = bincode::serialize(mr).context(Bincode)?;
	db.put(sha.trim().as_bytes(), bytes).context(Db)
}

/// Create a merge request, add it to the database, and post a comment stating the merge is
/// pending.
pub async fn wait_to_merge(
	state: &AppState,
	sha: &str,
	mr: &MergeRequest,
	msg: Option<&str>,
) -> Result<()> {
	register_merge_request(state, sha, mr).await?;

	let AppState { github_bot, .. } = state;

	let MergeRequest {
		owner,
		repo,
		number,
		..
	} = mr;

	let post_comment_result = github_bot
		.create_issue_comment(
			owner,
			repo,
			*number,
			msg.unwrap_or("Waiting for commit status."),
		)
		.await;
	if let Err(err) = post_comment_result {
		log::error!("Error posting comment: {}", err);
	}

	Ok(())
}

pub fn cleanup_merged_pr(state: &AppState, pr: &PullRequest) -> Result<bool> {
	if !pr.merged {
		return Ok(false);
	}

	cleanup_pr(
		state,
		&pr.head.sha,
		&pr.base.repo.owner.login,
		&pr.base.repo.name,
		pr.number,
	)
	.map(|_| true)
}

pub fn cleanup_pr(
	state: &AppState,
	key_to_guarantee_deleted: &str,
	owner: &str,
	repo: &str,
	number: i64,
) -> Result<()> {
	let AppState { db, .. } = state;

	let iter = db.iterator(rocksdb::IteratorMode::Start);
	for (key, value) in iter {
		let db_mr: MergeRequest =
			match bincode::deserialize(&value).context(Bincode) {
				Ok(mr) => mr,
				Err(err) => {
					log::error!(
						"Failed to deserialize {} during cleanup_pr due to {:?}",
						String::from_utf8_lossy(&key),
						err
					);
					continue;
				}
			};

		if db_mr.owner != owner || db_mr.repo != repo || db_mr.number != number
		{
			continue;
		}

		log::info!(
			"Cleaning up {:?} due to SHA {} of {}/{}#{}",
			db_mr,
			key_to_guarantee_deleted,
			owner,
			repo,
			number
		);

		if let Err(err) = db.delete(&key) {
			log::error!(
				"Failed to delete {} during cleanup_pr due to {:?}",
				String::from_utf8_lossy(&key),
				err
			);
		}
	}

	// Sanity-check: the key should have actually been deleted
	if let Some(_) = db.get(key_to_guarantee_deleted).context(Db)? {
		return Err(Error::Message {
			msg: format!(
				"Key {} was not deleted from the database",
				key_to_guarantee_deleted
			),
		});
	}

	Ok(())
}

/// Send a merge request.
/// It might recursively call itself when attempting to solve a merge error after something
/// meaningful happens.
#[async_recursion]
pub async fn merge(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
	created_approval_id: Option<i64>,
) -> Result<Result<()>> {
	if cleanup_merged_pr(state, pr)? {
		return Ok(Ok(()));
	}

	let AppState { github_bot, .. } = state;

	if let Err(err) = async {
		let err = match github_bot
			.merge_pull_request(
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
				&pr.head.sha,
			)
			.await
		{
			Ok(_) => {
				log::info!("{} merged successfully.", pr.html_url);
				if let Err(err) = cleanup_pr(
					state,
					&pr.head.sha,
					&pr.base.repo.owner.login,
					&pr.base.repo.name,
					pr.number,
				) {
					log::error!("Failed to cleanup PR on the database after merge: {}", err);
				};
				return Ok(());
			}
			Err(err) => err,
		};

		let msg = match err {
			Error::Response {
				ref status,
				ref body,
			} if *status == StatusCode::METHOD_NOT_ALLOWED => {
				match body.get("message") {
					Some(msg) => match msg.as_str() {
						Some(msg) => msg,
						None => {
							log::error!("Expected \"message\" of Github API merge failure response to be a string");
							return Err(err);
						},
					},
					None => {
						log::error!("Expected \"message\" of Github API merge failure response to be available");
						return Err(err);
					},
				}
			}
			_ => return Err(err),
		};

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
			.find(&msg.to_string())
			.is_some()
		{
			// This problem will be solved automatically when all the required statuses are delivered, thus
			// it can be ignored here
			log::info!(
				"Ignoring merge failure due to pending required status; message: {}",
				msg
			);
			return Err(Error::MergeFailureWillBeSolvedLater { msg: msg.to_string() });
		}

		// From this point onwards we'll attempt to recover from "Missing N approvals case"

		// If the bot has already approved once, the missing approval can't be fulfilled by going
		// further, so exit early.
		if created_approval_id.is_some() {
			log::info!("Failed to recover from merge error even after granting the bot approval");
			return Err(Error::Message { msg: msg.to_string() })
		}

		let min_approvals_required = {
			// Matches the following
			// - "At least N approving reviews are required by reviewers with write access."
			let insufficient_approval_quota_matcher =
				RegexBuilder::new(r"([[:digit:]]+).*approving\s+reviews?\s+(is|are)\s+required")
					.case_insensitive(true)
					.build()
					.unwrap();

			match insufficient_approval_quota_matcher.captures(&msg.to_string()) {
				Some(matches) => matches
					.get(1)
					.unwrap()
					.as_str()
					.parse::<usize>()
					.unwrap(),
				None => return Err(Error::Message { msg: msg.to_string() })
			}
		};

		let requester_role = match check_merge_is_allowed(
			state,
			pr,
			requested_by,
			Some(min_approvals_required),
		)
		.await
		{
			Ok(requester_role) => match requester_role {
				Some(requester_role) => requester_role,
				None => return Err(Error::Message {
					msg: "Requester's approval is not enough to make the PR mergeable".to_string()
				}),
			},
			Err(err) => {
				log::info!("Failed to get requested role for approval of {} due to {}", pr.html_url, err);
				return Err(Error::Message { msg: msg.to_string() });
			}
		};

		if let Err(err) = github_bot
			.create_issue_comment(
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
				&format!(
					"Bot will approve on the behalf of @{}, since they are {}, in an attempt to reach the minimum approval count",
					requested_by,
					requester_role,
				),
			)
			.await
		{
			log::error!("Failed to post comment on bot approval of {} due to {}", pr.html_url, err);
		}

		let review = match github_bot.approve_merge_request(
			&pr.base.repo.owner.login,
			&pr.base.repo.name,
			pr.number
		).await {
			Ok(review) => review,
			Err(err) => {
				log::error!(
					"Failed to create a review for approving the merge request {} due to {:?}",
					pr.html_url,
					err
				);
				return Err(Error::Message { msg: msg.to_string() });
			}
		};

		merge(
			state,
			pr,
			requested_by,
			Some(review.id)
		).await??;

		Ok(())
	}
	.await
	{
		if let Some(approval_id) = created_approval_id {
			if let Err(clear_err) = github_bot
				.clear_merge_request_approval(
					&pr.base.repo.owner.login,
					&pr.base.repo.name,
					pr.number,
					approval_id,
				)
				.await
			{
				log::error!(
					"Failed to cleanup a bot review in {} due to {}",
					pr.html_url,
					clear_err
				)
			}
		}
		return Err(err);
	}

	Ok(Ok(()))
}

fn get_troubleshoot_msg() -> String {
	return format!(
		"Merge failed. Check out the [criteria for merge](https://github.com/paritytech/parity-processbot#criteria-for-merge). If you're not meeting the approval count, check if the approvers are members of {} or {}.",
		SUBSTRATE_TEAM_LEADS_GROUP,
		CORE_DEVS_GROUP
	);
}

fn display_errors_along_the_way(errors: Option<Vec<String>>) -> String {
	errors
		.map(|errors| {
			if errors.is_empty() {
				"".to_string()
			} else {
				format!(
					"The following errors **might** have affected the outcome of this attempt:\n{}",
					errors.iter().map(|e| format!("- {}", e)).join("\n")
				)
			}
		})
		.unwrap_or_else(|| "".to_string())
}

fn format_error(err: Error) -> String {
	match err {
		Error::ProcessInfo { errors } => {
			format!(
				PROCESS_INFO_ERROR_TEMPLATE!(),
				PROCESS_FILE,
				display_errors_along_the_way(errors),
				get_troubleshoot_msg()
			)
		}
		Error::Approval { errors } => format!(
			"Approval criteria was not satisfied.\n\n{}\n\n{}",
			display_errors_along_the_way(errors),
			get_troubleshoot_msg()
		),
		Error::Response {
			ref body,
			ref status,
		} => format!(
			"Response error (status {}): <pre><code>{}</code></pre>",
			status,
			html_escape::encode_safe(&body.to_string())
		),
		_ => format!("{}", err),
	}
}

pub async fn handle_error(
	merge_cancel_outcome: MergeCancelOutcome,
	err: Error,
	state: &AppState,
) {
	log::info!("handle_error: {}", err);
	match err {
		Error::MergeFailureWillBeSolvedLater { .. } => (),
		err => match err {
			Error::WithIssue {
				source,
				issue: (owner, repo, number),
				..
			} => match *source {
				Error::MergeFailureWillBeSolvedLater { .. } => (),
				err => {
					let msg = {
						let description = format_error(err);
						let caption = match merge_cancel_outcome {
							MergeCancelOutcome::ShaNotFound  => "",
							MergeCancelOutcome::WasCancelled => "Merge cancelled due to error.",
							MergeCancelOutcome::WasNotCancelled => "Some error happened, but the merge was not cancelled (likely due to a bug).",
						};
						format!("{} Error: {}", caption, description)
					};
					if let Err(comment_post_err) = state
						.github_bot
						.create_issue_comment(&owner, &repo, number, &msg)
						.await
					{
						log::error!(
							"Error posting comment: {}",
							comment_post_err
						);
					}
				}
			},
			_ => (),
		},
	}
}

async fn get_match_from_registered_companions(
	state: &AppState,
	sha: &str,
) -> Result<Option<((String, MergeRequest), PullRequest)>> {
	let AppState { db, github_bot, .. } = state;

	let iter = db.iterator(rocksdb::IteratorMode::Start);
	for (parent_sha, value) in iter {
		let parent: MergeRequest =
			bincode::deserialize(&value).context(Bincode)?;

		let companion_children = match parent.companion_children {
			Some(ref companion_children) => companion_children,
			_ => continue,
		};

		for child in companion_children {
			let pr = github_bot
				.pull_request(&child.owner, &child.repo, child.number)
				.await?;
			// TODO: consider that a PR could be a companion of multiple parents
			if &pr.head.sha == sha {
				return Ok(Some((
					(String::from_utf8_lossy(&parent_sha).to_string(), parent),
					pr,
				)));
			}
		}
	}

	Ok(None)
}
