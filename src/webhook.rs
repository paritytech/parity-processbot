use futures::StreamExt;
use futures_util::future::TryFutureExt;
use hyper::{http::StatusCode, Body, Request, Response};
use itertools::Itertools;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::OptionExt;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
	companion::*, config::BotConfig, constants::*, error::*, github::*,
	github_bot::GithubBot, matrix_bot::MatrixBot, process, Result,
};

pub const BAMBOO_DATA_KEY: &str = "BAMBOO_DATA";
pub const CORE_DEVS_KEY: &str = "CORE_DEVS";

pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub bot_config: BotConfig,
	pub webhook_secret: String,
	pub environment: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	owner: String,
	repo_name: String,
	number: i64,
	html_url: String,
	requested_by: String,
}

fn verify(
	secret: &[u8],
	msg: &[u8],
	signature: &[u8],
) -> std::result::Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	hmac::verify(&key, msg, signature)
}

pub async fn webhook(
	req: Request<Body>,
	state: Arc<Mutex<AppState>>,
) -> Result<Response<Body>> {
	if req.uri().path() == "/webhook" {
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
		match webhook_inner(req, state).await {
			Err(e) => {
				log::error!("{}", e);
			}
			_ => {}
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

	if let Err(e) = handle_payload(payload, state).await {
		log::error!("{:?}", e);
	}

	Ok(())
}

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
			check_run:
				CheckRun {
					status,
					head_sha,
					pull_requests,
					..
				},
			..
		} => handle_check(status, head_sha, pull_requests, state).await,
		_event => Ok(()),
	}
}

async fn get_pr(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	number: i64,
) -> Result<PullRequest> {
	match github_bot.pull_request(&owner, &repo_name, number).await {
		Err(e) => {
			log::error!("Error getting PR: {}", e);
			let _ = github_bot
				.create_issue_comment(
					&owner,
					&repo_name,
					number,
					"Merge failed due to network error; see logs.",
				)
				.await
				.map_err(|e| {
					log::error!("Error posting comment: {}", e);
				});
			Err(e)
		}
		Ok(pr) => Ok(pr),
	}
}

async fn handle_check(
	status: String,
	commit_sha: String,
	pull_requests: Vec<CheckRunPR>,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	if status == "completed".to_string() {
		match db.get(commit_sha.trim().as_bytes()) {
			Ok(Some(b)) => {
				log::info!("Check {} for commit {}", status, commit_sha);
				match bincode::deserialize(&b) {
					Ok(m) => {
						log::info!("Deserialized merge request: {:?}", m);
						let MergeRequest {
							owner,
							repo_name,
							number,
							html_url,
							requested_by,
						} = m;
						if pull_requests
							.iter()
							.find(|pr| pr.number == number)
							.is_some()
						{
							if let Ok(pr) =
								get_pr(github_bot, &owner, &repo_name, number)
									.await
							{
								checks_and_status(
									github_bot,
									&owner,
									&repo_name,
									&commit_sha,
									&pr,
									&html_url,
									db,
									bot_config,
									&requested_by,
								)
								.await;
							}
						}
					}
					Err(e) => {
						log::error!("Error deserializing merge request: {}", e);
					}
				}
			}
			Ok(None) => {
				// sha not stored for merge
			}
			Err(e) => {
				log::error!(
					"Error reading from db (key: {}): {}",
					commit_sha,
					e
				);
			}
		}
	}

	Ok(())
}

async fn handle_status(
	commit_sha: String,
	status: StatusState,
	state: &AppState,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	if status != StatusState::Pending {
		match db.get(commit_sha.trim().as_bytes()) {
			Ok(Some(b)) => {
				log::info!("Status {:?} for commit {}", status, commit_sha);
				match bincode::deserialize(&b) {
					Ok(m) => {
						log::info!("Deserialized merge request: {:?}", m);
						let MergeRequest {
							owner,
							repo_name,
							number,
							html_url,
							requested_by,
						} = m;
						if let Ok(pr) =
							get_pr(github_bot, &owner, &repo_name, number).await
						{
							checks_and_status(
								github_bot,
								&owner,
								&repo_name,
								&commit_sha,
								&pr,
								&html_url,
								db,
								bot_config,
								&requested_by,
							)
							.await;
						}
					}
					Err(e) => {
						log::error!("Error deserializing merge request: {}", e);
					}
				}
			}
			Ok(None) => {
				// commit not stored for merge
			}
			Err(e) => {
				log::error!(
					"Error reading from db (key: {}): {}",
					commit_sha,
					e
				);
			}
		}
	}
	Ok(())
}

async fn checks_and_status(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	commit_sha: &str,
	pr: &PullRequest,
	html_url: &str,
	db: &DB,
	bot_config: &BotConfig,
	requested_by: &str,
) {
	// Head sha should not have changed since request was made.
	if commit_sha == pr.head.sha {
		log::info!("Commit sha {} matches head of {}", commit_sha, html_url);

		// Delay after status hook to avoid false success
		tokio::time::delay_for(std::time::Duration::from_millis(1000)).await;

		let checks =
			github_bot.check_runs(&owner, &repo_name, &commit_sha).await;
		log::info!("{:?}", checks);
		match checks {
			Ok(checks) => {
				if checks
					.check_runs
					.iter()
					.all(|r| r.conclusion == Some("success".to_string()))
				{
					log::info!("All checks success");
					let status = github_bot
						.status(&owner, &repo_name, &commit_sha)
						.await;
					log::info!("{:?}", status);
					match status {
						Ok(CombinedStatus {
							state: StatusState::Success,
							..
						}) => {
							log::info!(
								"{} is green; attempting merge.",
								html_url
							);
							continue_merge(
								github_bot,
								&owner,
								&repo_name,
								&pr,
								db,
								&bot_config,
								&requested_by,
							)
							.await
						}
						Ok(CombinedStatus {
							state: StatusState::Failure,
							statuses,
							..
						}) => {
							match dbg!(
								github_bot
									.contents(
										owner,
										repo_name,
										".gitlab-ci.yaml",
										&pr.head.ref_field,
									)
									.await
							) {
								Ok(ci) => {
									if statuses.iter().any(
										|Status { state, context, .. }| {
											state == &StatusState::Failure
												&& !status_failure_allowed(
													&ci.content,
													&context,
												)
										},
									) {
										log::info!(
											"{} failed a required status check.",
											html_url
										);
										status_failure(
											&github_bot,
											&owner,
											&repo_name,
											pr.number,
											&html_url,
											&commit_sha,
											db,
										)
										.await
									} else if statuses.iter().all(
										|Status { state, context, .. }| {
											state == &StatusState::Success
												|| (state
													== &StatusState::Failure
													&& status_failure_allowed(
														&ci.content,
														&context,
													))
										},
									) {
										log::info!(
                                            "{} required statuses are green; attempting merge.",
                                            html_url
                                        );
										continue_merge(
											github_bot,
											&owner,
											&repo_name,
											&pr,
											db,
											&bot_config,
											&requested_by,
										)
										.await
									} else if statuses.iter().all(
										|Status { state, context, .. }| {
											state == &StatusState::Success
												|| state
													== &StatusState::Pending || (state
												== &StatusState::Failure
												&& status_failure_allowed(
													&ci.content,
													&context,
												))
										},
									) {
										log::info!("{} is pending.", html_url);
									}
								}
								Err(e) => {
									log::error!(
										"Error getting .gitlab-ci.yaml: {}",
										e
									);
								}
							}
						}
						Ok(CombinedStatus {
							state: StatusState::Error,
							..
						}) => {
							log::info!("{} status error.", html_url);
							status_failure(
								&github_bot,
								&owner,
								&repo_name,
								pr.number,
								&html_url,
								&commit_sha,
								db,
							)
							.await
						}
						Ok(CombinedStatus {
							state: StatusState::Pending,
							..
						}) => {
							log::info!("{} is pending.", html_url);
						}
						Err(e) => {
							log::error!("Error getting combined status: {}", e);
							// Notify people of merge failure.
							let _ = github_bot.create_issue_comment(
                                    &owner,
                                    &repo_name,
                                    pr.number,
                                    "Merge failed due to network error; see logs for details.",
                                )
                                .await
                                .map_err(|e| {
                                    log::error!(
                                        "Error posting comment: {}",
                                        e
                                    );
                                });
							// Clean db.
							let _ = db
									.delete(commit_sha.as_bytes())
									.map_err(|e| {
										log::error!(
                                            "Error deleting merge request from db: {}",
                                            e
                                        );
									});
						}
					}
				} else if checks
					.check_runs
					.iter()
					.all(|r| r.status == "completed".to_string())
				{
					log::info!("{} checks were unsuccessful", html_url);
					// Notify people of merge failure.
					let _ = github_bot
						.create_issue_comment(
							owner,
							&repo_name,
							pr.number,
							"Checks were unsuccessful; aborting merge.",
						)
						.await
						.map_err(|e| {
							log::error!("Error posting comment: {}", e);
						});
					// Clean db.
					let _ = db.delete(commit_sha.as_bytes()).map_err(|e| {
						log::error!(
							"Error deleting merge request from db: {}",
							e
						);
					});
				} else {
					log::info!("{} checks incomplete", html_url);
				}
			}
			Err(e) => {
				log::error!("Error getting check runs: {}", e);
			}
		}
	} else {
		// Head sha has changed since merge request.
		log::info!(
			"Head sha has changed since merge was requested on {}",
			html_url
		);
		// Notify people of merge failure.
		let _ = github_bot.create_issue_comment(
            &owner,
            &repo_name,
            pr.number,
            "Head SHA has changed since merge was requested; aborting merge.",
        )
        .await
        .map_err(|e| {
            log::error!(
                "Error posting comment: {}",
                e
            );
        });
		// Clean db.
		let _ = db.delete(commit_sha.as_bytes()).map_err(|e| {
			log::error!("Error deleting merge request from db: {}", e);
		});
	}
}

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

	if body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim() {
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			requested_by
		);

		if requested_by != "parity-processbot" {
			// Check the user is a member of the org
			let member = github_bot.org_member(&owner, &requested_by).await;
			if let Err(e) = member {
				log::error!("Error getting organization membership: {:?}", e);
				let _ = github_bot
                .create_issue_comment(
                    owner,
                    &repo_name,
                    number,
                    "Error getting organization membership; see logs for details.",
                )
                .await
                .map_err(|e| {
                    log::error!(
                        "Error posting comment: {}",
                        e
                    );
                });
				return Ok(());
			} else if member.unwrap() != 204 {
				log::warn!(
					"Merge requested by {}, who is not a member of {}.",
					requested_by,
					owner
				);
				let _ = github_bot
					.create_issue_comment(
						owner,
						&repo_name,
						number,
						&format!(
							"Only members of {} can request merges.",
							owner
						),
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
				return Ok(());
			}
		}

		// Fetch the pr to get all fields (eg. mergeable).
		match github_bot.pull_request(owner, &repo_name, number).await {
			Ok(pr) => {
				match github_bot.status(owner, &repo_name, &pr.head.sha).await {
					Ok(CombinedStatus {
						state: StatusState::Success,
						..
					}) => {
						log::info!("{} is green.", html_url);
						let checks = github_bot
							.check_runs(&owner, &repo_name, &pr.head.sha)
							.await;
						log::info!("{:?}", checks);
						match checks {
							Ok(checks) => {
								if checks.check_runs.iter().all(|r| {
									r.conclusion == Some("success".to_string())
								}) {
									log::info!(
										"{} checks successful.",
										html_url
									);
									let m = MergeRequest {
										owner: owner.to_string(),
										repo_name: repo_name.clone(),
										number: pr.number,
										html_url: pr.html_url.clone(),
										requested_by: requested_by.to_string(),
									};
									log::info!(
										"Serializing merge request: {:?}",
										m
									);
									match bincode::serialize(&m) {
										Ok(bytes) => {
											log::info!("Writing merge request to db (head ref: {})", pr.head.ref_field);
											match db.put(
												pr.head.sha.trim().as_bytes(),
												bytes,
											) {
												Ok(_) => {
													log::info!("Trying merge.");
													let _ = github_bot
                                                        .create_issue_comment(
                                                            owner,
                                                            &repo_name,
                                                            pr.number,
                                                            "Trying merge.",
                                                        )
                                                        .await
                                                        .map_err(|e| {
                                                            log::error!(
                                                                "Error posting comment: {}",
                                                                e
                                                            );
                                                        });
													continue_merge(
														github_bot,
														owner,
														&repo_name,
														&pr,
														db,
														&bot_config,
														&requested_by,
													)
													.await;
												}
												Err(e) => {
													log::error!("Error adding merge request to db: {}", e);
													let _ = github_bot.create_issue_comment(
                                                        owner,
                                                        &repo_name,
                                                        pr.number,
                                                        "Merge failed due to a database error.",
                                                    )
                                                    .await
                                                    .map_err(|e| {
                                                        log::error!(
                                                            "Error posting comment: {}",
                                                            e
                                                        );
                                                    });
												}
											}
										}
										Err(e) => {
											log::error!("Error serializing merge request: {}", e);
											let _ = github_bot.create_issue_comment(
                                                owner,
                                                &repo_name,
                                                pr.number,
                                                "Merge failed due to a serialization error.",
                                            )
                                            .await
                                            .map_err(|e| {
                                                log::error!(
                                                    "Error posting comment: {}",
                                                    e
                                                );
                                            });
										}
									}
								} else if checks.check_runs.iter().all(|r| {
									r.status == "completed".to_string()
								}) {
									log::info!(
										"{} checks were unsuccessful.",
										html_url
									);
									// Notify people of merge failure.
									let _ = github_bot.create_issue_comment(
                                        owner,
                                        &repo_name,
                                        pr.number,
                                        "Checks were unsuccessful; aborting merge.",
                                    )
                                    .await
                                    .map_err(|e| {
                                        log::error!(
                                            "Error posting comment: {}",
                                            e
                                        );
                                    });
								} else {
									log::info!(
										"{} checks incomplete.",
										html_url
									);
									let m = MergeRequest {
										owner: owner.to_string(),
										repo_name: repo_name.clone(),
										number: pr.number,
										html_url: pr.html_url.clone(),
										requested_by: requested_by.to_string(),
									};
									log::info!(
										"Serializing merge request: {:?}",
										m
									);
									match bincode::serialize(&m) {
										Ok(bytes) => {
											log::info!("Writing merge request to db (head ref: {})", pr.head.ref_field);
											match db.put(
												pr.head.sha.trim().as_bytes(),
												bytes,
											) {
												Ok(_) => {
													log::info!("Waiting for commit status...");
													let _ = github_bot
                                                        .create_issue_comment(
                                                            owner,
                                                            &repo_name,
                                                            pr.number,
                                                            "Waiting for commit status...",
                                                        )
                                                        .await
                                                        .map_err(|e| {
                                                            log::error!(
                                                                "Error posting comment: {}",
                                                                e
                                                            );
                                                        });
												}
												Err(e) => {
													log::error!("Error adding merge request to db: {}", e);
													let _ = github_bot.create_issue_comment(
                                                        owner,
                                                        &repo_name,
                                                        pr.number,
                                                        "Merge failed due to a database error.",
                                                    )
                                                    .await
                                                    .map_err(|e| {
                                                        log::error!(
                                                            "Error posting comment: {}",
                                                            e
                                                        );
                                                    });
												}
											}
										}
										Err(e) => {
											log::error!("Error serializing merge request: {}", e);
											let _ = github_bot.create_issue_comment(
                                                owner,
                                                &repo_name,
                                                pr.number,
                                                "Merge failed due to serialization error.",
                                            )
                                            .await
                                            .map_err(|e| {
                                                log::error!(
                                                    "Error posting comment: {}",
                                                    e
                                                );
                                            });
										}
									}
								}
							}
							Err(e) => {
								log::error!("Error getting check runs: {}", e);
							}
						}
					}
					Ok(CombinedStatus {
						state: StatusState::Pending,
						..
					}) => {
						log::info!("Status pending for PR {}", pr.html_url);
						let m = MergeRequest {
							owner: owner.to_string(),
							repo_name: repo_name.clone(),
							number: pr.number,
							html_url: pr.html_url.clone(),
							requested_by: requested_by.to_string(),
						};
						log::info!("Serializing merge request: {:?}", m);
						match bincode::serialize(&m) {
							Ok(bytes) => {
								log::info!("Writing merge request to db (head ref: {})", pr.head.ref_field);
								match db
									.put(pr.head.sha.trim().as_bytes(), bytes)
								{
									Ok(_) => {
										log::info!(
											"Waiting for commit status..."
										);
										let _ = github_bot
											.create_issue_comment(
												owner,
												&repo_name,
												pr.number,
												"Waiting for commit status...",
											)
											.await
											.map_err(|e| {
												log::error!(
													"Error posting comment: {}",
													e
												);
											});
									}
									Err(e) => {
										log::error!("Error adding merge request to db: {}", e);
										let _ = github_bot.create_issue_comment(
                                            owner,
                                            &repo_name,
                                            pr.number,
                                            "Merge failed due to a database error.",
                                        )
                                        .await
                                        .map_err(|e| {
                                            log::error!(
                                                "Error posting comment: {}",
                                                e
                                            );
                                        });
									}
								}
							}
							Err(e) => {
								log::error!(
									"Error serializing merge request: {}",
									e
								);
								let _ = github_bot.create_issue_comment(
                                    owner,
                                    &repo_name,
                                    pr.number,
                                    "Merge failed due to a serialization error.",
                                )
                                .await
                                .map_err(|e| {
                                    log::error!(
                                        "Error posting comment: {}",
                                        e
                                    );
                                });
							}
						}
					}
					Ok(CombinedStatus {
						state: StatusState::Failure,
						statuses,
						..
					}) => {
						match dbg!(
							github_bot
								.contents(
									owner,
									&repo_name,
									".gitlab-ci.yaml",
									&pr.head.ref_field,
								)
								.await
						) {
							Ok(ci) => {
								if statuses.iter().any(
									|Status { state, context, .. }| {
										state == &StatusState::Failure
											&& !status_failure_allowed(
												&ci.content,
												&context,
											)
									},
								) {
									log::info!(
										"{} failed a required status check.",
										html_url
									);
									status_failure(
										&github_bot,
										&owner,
										&repo_name,
										pr.number,
										&pr.html_url,
										&pr.head.sha,
										db,
									)
									.await
								} else if statuses.iter().all(
									|Status { state, context, .. }| {
										state == &StatusState::Success
											|| (state == &StatusState::Failure
												&& status_failure_allowed(
													&ci.content,
													&context,
												))
									},
								) {
									log::info!(
										"{} required statuses are green.",
										html_url
									);
									let checks = github_bot
										.check_runs(
											&owner,
											&repo_name,
											&pr.head.sha,
										)
										.await;
									log::info!("{:?}", checks);
									match checks {
										Ok(checks) => {
											if checks.check_runs.iter().all(
												|r| {
													r.conclusion
														== Some(
															"success"
																.to_string(),
														)
												},
											) {
												log::info!(
													"{} checks successful.",
													html_url
												);
												let m = MergeRequest {
													owner: owner.to_string(),
													repo_name: repo_name
														.clone(),
													number: pr.number,
													html_url: pr
														.html_url
														.clone(),
													requested_by: requested_by
														.to_string(),
												};
												log::info!(
                                                    "Serializing merge request: {:?}",
                                                    m
                                                );
												match bincode::serialize(&m) {
													Ok(bytes) => {
														log::info!("Writing merge request to db (head ref: {})", pr.head.ref_field);
														match db.put(
															pr.head
																.sha
																.trim()
																.as_bytes(),
															bytes,
														) {
															Ok(_) => {
																log::info!("Trying merge.");
																let _ = github_bot
                                                                    .create_issue_comment(
                                                                        owner,
                                                                        &repo_name,
                                                                        pr.number,
                                                                        "Trying merge.",
                                                                    )
                                                                    .await
                                                                    .map_err(|e| {
                                                                        log::error!(
                                                                            "Error posting comment: {}",
                                                                            e
                                                                        );
                                                                    });
																continue_merge(
                                                                    github_bot,
                                                                    owner,
                                                                    &repo_name,
                                                                    &pr,
                                                                    db,
                                                                    &bot_config,
                                                                    &requested_by,
                                                                )
                                                                .await;
															}
															Err(e) => {
																log::error!("Error adding merge request to db: {}", e);
																let _ = github_bot.create_issue_comment(
                                                                    owner,
                                                                    &repo_name,
                                                                    pr.number,
                                                                    "Merge failed due to a database error.",
                                                                )
                                                                .await
                                                                .map_err(|e| {
                                                                    log::error!(
                                                                        "Error posting comment: {}",
                                                                        e
                                                                    );
                                                                });
															}
														}
													}
													Err(e) => {
														log::error!("Error serializing merge request: {}", e);
														let _ = github_bot.create_issue_comment(
                                                            owner,
                                                            &repo_name,
                                                            pr.number,
                                                            "Merge failed due to a serialization error.",
                                                        )
                                                        .await
                                                        .map_err(|e| {
                                                            log::error!(
                                                                "Error posting comment: {}",
                                                                e
                                                            );
                                                        });
													}
												}
											} else if checks
												.check_runs
												.iter()
												.all(|r| {
													r.status
														== "completed"
															.to_string()
												}) {
												log::info!(
                                                    "{} checks were unsuccessful.",
                                                    html_url
                                                );
												// Notify people of merge failure.
												let _ = github_bot.create_issue_comment(
                                                    owner,
                                                    &repo_name,
                                                    pr.number,
                                                    "Checks were unsuccessful; aborting merge.",
                                                )
                                                .await
                                                .map_err(|e| {
                                                    log::error!(
                                                        "Error posting comment: {}",
                                                        e
                                                    );
                                                });
											} else {
												log::info!(
													"{} checks incomplete.",
													html_url
												);
												let m = MergeRequest {
													owner: owner.to_string(),
													repo_name: repo_name
														.clone(),
													number: pr.number,
													html_url: pr
														.html_url
														.clone(),
													requested_by: requested_by
														.to_string(),
												};
												log::info!(
                                                    "Serializing merge request: {:?}",
                                                    m
                                                );
												match bincode::serialize(&m) {
													Ok(bytes) => {
														log::info!("Writing merge request to db (head ref: {})", pr.head.ref_field);
														match db.put(
															pr.head
																.sha
																.trim()
																.as_bytes(),
															bytes,
														) {
															Ok(_) => {
																log::info!("Waiting for commit status...");
																let _ = github_bot
                                                                    .create_issue_comment(
                                                                        owner,
                                                                        &repo_name,
                                                                        pr.number,
                                                                        "Waiting for commit status...",
                                                                    )
                                                                    .await
                                                                    .map_err(|e| {
                                                                        log::error!(
                                                                            "Error posting comment: {}",
                                                                            e
                                                                        );
                                                                    });
															}
															Err(e) => {
																log::error!("Error adding merge request to db: {}", e);
																let _ = github_bot.create_issue_comment(
                                                                    owner,
                                                                    &repo_name,
                                                                    pr.number,
                                                                    "Merge failed due to a database error.",
                                                                )
                                                                .await
                                                                .map_err(|e| {
                                                                    log::error!(
                                                                        "Error posting comment: {}",
                                                                        e
                                                                    );
                                                                });
															}
														}
													}
													Err(e) => {
														log::error!("Error serializing merge request: {}", e);
														let _ = github_bot.create_issue_comment(
                                                            owner,
                                                            &repo_name,
                                                            pr.number,
                                                            "Merge failed due to serialization error.",
                                                        )
                                                        .await
                                                        .map_err(|e| {
                                                            log::error!(
                                                                "Error posting comment: {}",
                                                                e
                                                            );
                                                        });
													}
												}
											}
										}
										Err(e) => {
											log::error!(
												"Error getting check runs: {}",
												e
											);
										}
									}
								} else if statuses.iter().all(
									|Status { state, context, .. }| {
										state == &StatusState::Success
											|| state == &StatusState::Pending
											|| (state == &StatusState::Failure
												&& status_failure_allowed(
													&ci.content,
													&context,
												))
									},
								) {
									log::info!("{} is pending.", html_url);
								}
							}
							Err(e) => {
								log::error!(
									"Error getting .gitlab-ci.yaml: {}",
									e
								);
							}
						}
					}
					Ok(CombinedStatus {
						state: StatusState::Error,
						..
					}) => {
						log::info!("{} status error.", html_url);
						status_failure(
							&github_bot,
							&owner,
							&repo_name,
							pr.number,
							&pr.html_url,
							&pr.head.sha,
							db,
						)
						.await
					}
					Err(e) => {
						log::error!("Error getting PR status: {}", e);
						// Notify people of merge failure.
						let _ = github_bot
							.create_issue_comment(
								owner,
								&repo_name,
								pr.number,
								"Merge failed due to a network error.",
							)
							.await
							.map_err(|e| {
								log::error!("Error posting comment: {}", e);
							});
						// Clean db.
						let _ =
							db.delete(pr.head.sha.as_bytes()).map_err(|e| {
								log::error!(
									"Error deleting merge request from db: {}",
									e
								);
							});
					}
				}
			}
			Err(e) => {
				log::error!("Error getting PR: {}", e);
				let _ = github_bot
					.create_issue_comment(
						owner,
						&repo_name,
						number,
						"Merge failed due to a network error.",
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
		}
	} else if body.to_lowercase().trim()
		== AUTO_MERGE_CANCEL.to_lowercase().trim()
	{
		log::info!(
			"Received merge cancel for PR {} from user {}",
			html_url,
			requested_by
		);
		// Fetch the pr to get all fields (eg. mergeable).
		match github_bot.pull_request(owner, &repo_name, number).await {
			Ok(pr) => {
				log::info!("Deleting merge request for {}", &html_url);
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
				// Clean db.
				let _ = db.delete(pr.head.sha.as_bytes()).map_err(|e| {
					log::error!("Error deleting merge request from db: {}", e);
				});
			}
			Err(e) => {
				log::error!("Error getting PR: {}", e);
			}
		}
	} else if repo_name == "polkadot"
		&& body.to_lowercase().trim()
			== COMPARE_RELEASE_REQUEST.to_lowercase().trim()
	{
		log::info!(
			"Received diff request for PR {} from user {}",
			html_url,
			requested_by
		);
		// Fetch the pr to get all fields (eg. mergeable).
		match github_bot.pull_request(owner, &repo_name, number).await {
			Ok(pr) => {
				match github_bot.latest_release(owner, &repo_name).await {
					Ok(rel) => {
						match github_bot
							.tag(owner, &repo_name, &rel.tag_name)
							.await
						{
							Ok(release_tag) => {
								let release_substrate_commit = github_bot
									.substrate_commit_from_polkadot_commit(
										&release_tag.object.sha,
									)
									.await;
								let branch_substrate_commit = github_bot
									.substrate_commit_from_polkadot_commit(
										&pr.head.sha,
									)
									.await;
								if release_substrate_commit.is_ok()
									&& branch_substrate_commit.is_ok()
								{
									let link = github_bot.diff_url(
										owner,
										"substrate",
										&release_substrate_commit.unwrap(),
										&branch_substrate_commit.unwrap(),
									);
									log::info!(
										"Posting link to substrate diff: {}",
										&link
									);
									let _ = github_bot
										.create_issue_comment(
											owner, &repo_name, number, &link,
										)
										.await
										.map_err(|e| {
											log::error!(
												"Error posting comment: {}",
												e
											);
										});
								} else {
									if let Err(e) = release_substrate_commit {
										log::error!("Error getting substrate commit: {}", e);
									}
									if let Err(e) = branch_substrate_commit {
										log::error!("Error getting substrate commit: {}", e);
									}
									let _ = github_bot
                                        .create_issue_comment(
                                            owner,
                                            &repo_name,
                                            number,
                                            "Error getting substrate commit; see logs for details",
                                        )
                                        .await
                                        .map_err(|e| {
                                            log::error!(
                                                "Error posting comment: {}",
                                                e
                                            );
                                        });
								}
							}
							Err(e) => {
								log::error!("Error getting release tag: {}", e);
								let _ = github_bot.create_issue_comment(
                                    owner,
                                    &repo_name,
                                    number,
                                    "Failed getting latest release tag; see logs for details",
                                )
                                .await
                                .map_err(|e| {
                                    log::error!(
                                        "Error posting comment: {}",
                                        e
                                    );
                                });
							}
						}
					}
					Err(e) => {
						log::error!("Error getting latest release: {}", e);
						let _ = github_bot.create_issue_comment(
                            owner,
                            &repo_name,
                            number,
                            "Failed getting latest release; see logs for details",
                        )
                        .await
                        .map_err(|e| {
                            log::error!(
                                "Error posting comment: {}",
                                e
                            );
                        });
					}
				}
			}
			Err(e) => {
				log::error!("Error getting PR: {}", e);
				let _ = github_bot
					.create_issue_comment(
						owner,
						&repo_name,
						number,
						"Network error; see logs for details.",
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
		}
	}

	Ok(())
}

async fn continue_merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
	db: &DB,
	bot_config: &BotConfig,
	requested_by: &str,
) {
	let mergeable = pr.mergeable.unwrap_or(false);
	if !mergeable {
		log::info!("{} is unmergeable", pr.html_url);
		let _ = github_bot
			.create_issue_comment(
				owner,
				repo_name,
				pr.number,
				"PR is unmergeable",
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
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
			// MERGE
			//
			log::info!(
				"{} merge requested by a team lead; merging.",
				pr.html_url
			);
			merge(github_bot, owner, repo_name, pr).await;
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
			if core_approved {
				//
				// MERGE
				//
				log::info!("{} has core approval; merging.", pr.html_url);
				merge(github_bot, owner, repo_name, pr).await;
			} else {
				match process::get_process(
					github_bot, owner, repo_name, pr.number,
				)
				.await
				{
					Err(e) => {
						log::error!("Error getting process info: {}", e);
						// Without process info the merge cannot complete so
						// let people know.
						let _ = github_bot
                            .create_issue_comment(
                                owner,
                                repo_name,
                                pr.number,
                                "Merge failed to due error getting process info; see logs."
                            )
                            .await
                            .map_err(|e| {
                                log::error!("Error posting comment: {}", e);
                            });
					}
					Ok(process) => {
						let owner_approved = reviews
							.iter()
							.sorted_by_key(|r| r.submitted_at)
							.rev()
							.find(|r| process.is_owner(&r.user.login))
							.map_or(false, |r| {
								r.state == Some(ReviewState::Approved)
							});

						let owner_requested = process.is_owner(&requested_by);

						if owner_approved || owner_requested {
							//
							// MERGE
							//
							log::info!(
								"{} has owner approval; merging.",
								pr.html_url
							);
							merge(github_bot, owner, repo_name, pr).await;
						} else {
							if process.is_empty() {
								log::info!("{} lacks process info - it might not belong to a valid project column", pr.html_url);
								let _ = github_bot
                                .create_issue_comment(
                                    owner,
                                    repo_name,
                                    pr.number,
                                    "PR lacks process info - check that it belongs to a valid project column",
                                )
                                .await
                                .map_err(|e| {
                                    log::error!("Error posting comment: {}", e);
                                });
							} else {
								log::info!("{} lacks approval from the project owner or at least {} core developers", pr.html_url, min_reviewers);
								let _ = github_bot
                                .create_issue_comment(
                                    owner,
                                    repo_name,
                                    pr.number,
                                    &format!("PR lacks approval from the project owner or at least {} core developers", bot_config.min_reviewers),
                                )
                                .await
                                .map_err(|e| {
                                    log::error!("Error posting comment: {}", e);
                                });
							}
						}
					}
				}
			}
		}
	}

	// Clean db.
	let _ = db.delete(pr.head.sha.as_bytes()).map_err(|e| {
		log::error!("Error deleting from db: {}", e);
	});
}

/// Attempt merge and return `true` if successful, otherwise `false`.
async fn merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
) {
	if let Err(e) = github_bot
		.merge_pull_request(owner, repo_name, pr.number, &pr.head.sha)
		.await
		.map_err(|e| {
			e.map_issue(Some((
				owner.to_string(),
				repo_name.to_string(),
				pr.number,
			)))
		}) {
		log::error!("Error merging: {}", &e);
		match e {
			Error::Response {
				body: serde_json::Value::Object(m),
				..
			} => {
				let _ = github_bot
					.create_issue_comment(
						owner,
						repo_name,
						pr.number,
						&format!("Merge failed - `{}`", m["message"]),
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
			Error::Http { source, .. } => {
				let _ = github_bot
					.create_issue_comment(
						owner,
						repo_name,
						pr.number,
						&format!(
							"Merge failed due to a network error:\n\n{}",
							source
						),
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
			_ => {
				let _ = github_bot
					.create_issue_comment(
						owner,
						repo_name,
						pr.number,
						"Merge failed due to an unexpected error; see logs for details.",
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
		};
	} else {
		log::info!("{} merged successfully.", pr.html_url);
		if repo_name == "substrate" {
			log::info!("Checking for companion.");
			if let Some(body) = &pr.body {
				let _ = check_companion(github_bot, &body).await;
			} else {
				log::info!("No PR body found.");
			}
		}
	}
}

async fn check_companion(
	github_bot: &GithubBot,
	body: &str,
	//	head: &str,
) -> Result<()> {
	// check for link in pr body
	if let Some((comp_html_url, comp_owner, comp_repo, comp_number)) =
		companion_parse(&body)
	{
		log::info!("Found companion {}", comp_html_url);
		if let Ok(PullRequest {
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
		}) = get_pr(github_bot, &comp_owner, &comp_repo, comp_number).await
		{
			update_companion(
				github_bot,
				&comp_html_url,
				&comp_owner,
				&comp_repo,
				comp_number,
				&comp_head_owner,
				&comp_head_repo,
				&comp_head_branch,
			)
			.await?;
		}
	} else {
		log::info!("No companion found.");
	}

	Ok(())
}

async fn update_companion(
	github_bot: &GithubBot,
	comp_html_url: &str,
	comp_owner: &str,
	comp_repo: &str,
	comp_number: i64,
	comp_head_owner: &str,
	comp_head_repo: &str,
	comp_head_branch: &str,
) -> Result<()> {
	log::info!("Updating companion {}", comp_html_url);
	if let Err(e) = companion_update(
		github_bot,
		&comp_head_owner,
		&comp_head_repo,
		&comp_head_branch,
	)
	.await
	{
		log::error!("Error updating companion: {:?}", e);
		let _ = github_bot
			.create_issue_comment(
				&comp_owner,
				&comp_repo,
				comp_number,
				"Error updating Cargo.lock; see logs for details",
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
	} else {
		log::info!("Companion updated; requesting merge for {}", comp_html_url);
		let _ = github_bot
			.create_issue_comment(
				&comp_owner,
				&comp_repo,
				comp_number,
				"bot merge",
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
	}
	Ok(())
}

async fn status_failure(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	number: i64,
	html_url: &str,
	sha: &str,
	db: &DB,
) {
	log::info!("Status failure for PR {}", html_url);
	// Notify people of merge failure.
	let _ = github_bot
		.create_issue_comment(
			owner,
			repo_name,
			number,
			"Failed a required check; merge cancelled.",
		)
		.await
		.map_err(|e| {
			log::error!("Error posting comment: {}", e);
		});
	// Clean db.
	let _ = db.delete(sha.as_bytes()).map_err(|e| {
		log::error!("Error deleting from db: {}", e);
	});
}

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
