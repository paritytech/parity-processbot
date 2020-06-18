use anyhow::{Context, Result};
use futures::StreamExt;
use futures_util::future::TryFutureExt;
use hyper::{http::StatusCode, Body, Request, Response};
use itertools::Itertools;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
	config::BotConfig, constants::*, error::*, github::*,
	github_bot::GithubBot, matrix_bot::MatrixBot, process,
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
	pub test_repo: String,
}

pub async fn webhook(
	mut req: Request<Body>,
	state: parking_lot::Mutex<Arc<AppState>>,
) -> anyhow::Result<Response<Body>> {
	if req.uri().path() == "/webhook" {
		let state = Arc::clone(&state.lock());
		let mut msg_bytes = vec![];
		while let Some(item) = req.body_mut().next().await {
			msg_bytes.extend_from_slice(
				&item.context(format!(
					"Error getting bytes from request body"
				))?,
			);
		}

		let sig = req
			.headers()
			.get("x-hub-signature")
			.context(format!("Missing x-hub-signature"))?
			.to_str()
			.context(format!("Error parsing x-hub-signature"))?
			.replace("sha1=", "");
		let sig_bytes = base16::decode(sig.as_bytes())
			.context(format!("Error decoding x-hub-signature"))?;

		verify(
			state.webhook_secret.trim().as_bytes(),
			&msg_bytes,
			&sig_bytes,
		)
		.context(format!("Validation signature does not match"))?;

		let payload = serde_json::from_slice::<Payload>(&msg_bytes)
			.context(format!("Error parsing request body"))?;

		if let Err(e) = handle_payload(payload, state).await {
			log::error!("{:?}", e);
		}

		Response::builder()
			.status(StatusCode::OK)
			.body(Body::from(""))
			.context(format!("Error building response"))
	} else {
		Response::builder()
			.status(StatusCode::NOT_FOUND)
			.body(Body::from("Not found."))
			.context(format!("Error building response"))
	}
}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	owner: String,
	repo_name: String,
	number: i64,
	html_url: String,
	requested_by: String,
	branch: String,
}

fn verify(
	secret: &[u8],
	msg: &[u8],
	signature: &[u8],
) -> Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	hmac::verify(&key, msg, signature)
}

async fn handle_payload(payload: Payload, state: Arc<AppState>) -> Result<()> {
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
		Payload::CommitStatus { sha, branches, .. } => {
			handle_status(sha, branches, state).await
		}
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
		_event => {
			//			log::debug!("{:?}", event);
			Ok(())
		}
	}
}

async fn handle_check(
	status: String,
	commit_sha: String,
	pull_requests: Vec<CheckRunPR>,
	state: Arc<AppState>,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	if status == "completed".to_string() {
		match db.get(commit_sha.trim().as_bytes()) {
			Ok(Some(b)) => match bincode::deserialize(&b) {
				Ok(m) => {
					log::info!("Deserialized merge request: {:?}", m);
					let MergeRequest {
						owner,
						repo_name,
						number,
						html_url,
						requested_by,
						branch: _branch,
					} = m;
					if let Some(pr) =
						pull_requests.iter().find(|pr| pr.number == number)
					{
						checks_and_status(
							github_bot,
							&owner,
							&repo_name,
							&commit_sha,
							&pr.head.sha,
							number,
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
			},
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
	branches: Vec<Branch>,
	state: Arc<AppState>,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	for Branch {
		name: head_ref,
		commit: BranchCommit { sha: head_sha, .. },
		..
	} in &branches
	{
		match db.get(commit_sha.trim().as_bytes()) {
			Ok(Some(b)) => match bincode::deserialize(&b) {
				Ok(m) => {
					log::info!("Deserialized merge request: {:?}", m);
					let MergeRequest {
						owner,
						repo_name,
						number,
						html_url,
						requested_by,
						branch,
					} = m;
					if head_ref == &branch {
						checks_and_status(
							github_bot,
							&owner,
							&repo_name,
							&commit_sha,
							&head_sha,
							number,
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
			},
			Ok(None) => {
				// branch not stored for merge
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

async fn handle_comment(
	body: String,
	login: String,
	number: i64,
	html_url: String,
	repo_url: String,
	state: Arc<AppState>,
) -> Result<()> {
	let db = &state.db;
	let github_bot = &state.github_bot;
	let bot_config = &state.bot_config;

	let owner = GithubBot::owner_from_html_url(&html_url)
		.context(format!("Failed parsing owner in url: {}", html_url))?;

	let repo_name = repo_url
		.rsplit('/')
		.next()
		.map(|s| s.to_string())
		.context(format!("Failed parsing repo name in url: {}", repo_url))?;

	if body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim() {
		log::info!(
			"Received merge request for PR {} from user {}",
			html_url,
			login
		);
		// Fetch the pr to get all fields (eg. mergeable).
		match github_bot.pull_request(owner, &repo_name, number).await {
			Ok(pr) => {
				match github_bot
					.status(owner, &repo_name, &pr.head.sha)
					.await
					.map(|s| s.state)
				{
					Ok(StatusState::Success) => {
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
										requested_by: login.to_string(),
										branch: pr.head.ref_field.clone(),
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
														&github_bot,
														owner,
														&repo_name,
														&pr,
														db,
														&bot_config,
														&login,
													)
													.await;
												}
												Err(e) => {
													log::error!("Error adding merge request to db: {}", e);
													let _ = github_bot.create_issue_comment(
                                                        owner,
                                                        &repo_name,
                                                        pr.number,
                                                        "Auto-merge failed due to db error; see logs for details.",
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
                                                "Auto-merge failed due to serialization error; see logs for details.",
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
                                        "Checks were unsuccessful; cancelling merge.",
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
										requested_by: login.to_string(),
										branch: pr.head.ref_field.clone(),
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
                                                        "Auto-merge failed due to db error; see logs for details.",
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
                                                "Auto-merge failed due to serialization error; see logs for details.",
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
					Ok(StatusState::Pending) => {
						log::info!("Status pending for PR {}", pr.html_url);
						let m = MergeRequest {
							owner: owner.to_string(),
							repo_name: repo_name.clone(),
							number: pr.number,
							html_url: pr.html_url.clone(),
							requested_by: login.to_string(),
							branch: pr.head.ref_field.clone(),
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
                                            "Auto-merge failed due to db error; see logs for details.",
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
                                    "Auto-merge failed due to serialization error; see logs for details.",
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
					Ok(StatusState::Failure) | Ok(StatusState::Error) => {
						log::info!("{} failed status checks.", html_url);
						status_failure(
							&github_bot,
							owner,
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
						let _ = github_bot.create_issue_comment(
                            owner,
                            &repo_name,
                            pr.number,
                            "Auto-merge failed due to network error; see logs for details.",
                        )
                        .await
                        .map_err(|e| {
                            log::error!(
                                "Error posting comment: {}",
                                e
                            );
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
			}
		}
	} else if body.to_lowercase().trim()
		== AUTO_MERGE_CANCEL.to_lowercase().trim()
	{
		log::info!(
			"Received merge cancel for PR {} from user {}",
			html_url,
			login
		);
		// Fetch the pr to get all fields (eg. mergeable).
		match github_bot.pull_request(owner, &repo_name, number).await {
			Ok(pr) => {
				log::info!(
					"Deleting merge request for branch {}",
					&pr.head.ref_field
				);
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
			login
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
                                    "Failed getting latest release tag; see logs for details.",
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
                            "Failed getting latest release; see logs for details.",
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
	head_sha: &str,
	number: i64,
	html_url: &str,
	db: &DB,
	bot_config: &BotConfig,
	requested_by: &str,
) {
	let checks = github_bot.check_runs(&owner, &repo_name, &commit_sha).await;
	log::info!("{:?}", checks);
	match checks {
		Ok(checks) => {
			if checks
				.check_runs
				.iter()
				.all(|r| r.conclusion == Some("success".to_string()))
			{
				log::info!("All check runs success");
				let status =
					github_bot.status(&owner, &repo_name, &commit_sha).await;
				log::info!("{:?}", status);
				match status {
					Ok(CombinedStatus {
						state: StatusState::Success,
						..
					}) => {
						log::info!("Combined status success");
						// Head sha of branch should not have changed since request was
						// made.
						if commit_sha == head_sha {
							match github_bot
								.pull_request(&owner, &repo_name, number)
								.await
							{
								Ok(pr) => {
									log::info!(
										"{} is green; attempting merge.",
										html_url
									);
									continue_merge(
										&github_bot,
										&owner,
										&repo_name,
										&pr,
										db,
										&bot_config,
										&requested_by,
									)
									.await
								}
								Err(e) => {
									log::error!("Error getting PR: {}", e);
									// Notify people of merge failure.
									let _ = github_bot.create_issue_comment(
                                        &owner,
                                        &repo_name,
                                        number,
                                        "Auto-merge failed due to network error; see logs for details.",
                                    )
                                    .await
                                    .map_err(|e| {
                                        log::error!(
                                            "Error posting comment: {}",
                                            e
                                        );
                                    });
									// Clean db.
									let _ = db.delete(
                                        commit_sha.as_bytes(),
                                    ).map_err(|e| {
                                        log::error!(
                                            "Error deleting merge request from db: {}",
                                            e
                                        );
                                    });
								}
							}
						} else {
							// branch matches but head sha has changed since merge request
							log::info!(
                                "Head sha has changed since merge was requested on {}", html_url
                            );
							// Notify people of merge failure.
							let _ = github_bot.create_issue_comment(
                                &owner,
                                &repo_name,
                                number,
                                "Head SHA has changed since merge was requested; cancelling merge.",
                            )
                            .await
                            .map_err(|e| {
                                log::error!(
                                    "Error posting comment: {}",
                                    e
                                );
                            });
							// Clean db.
							let _ =
								db.delete(commit_sha.as_bytes()).map_err(|e| {
									log::error!(
                                        "Error deleting merge request from db: {}",
                                        e
                                    );
								});
						}
					}
					Ok(CombinedStatus {
						state: StatusState::Failure,
						..
					})
					| Ok(CombinedStatus {
						state: StatusState::Error,
						..
					}) => {
						log::info!("{} failed status checks.", html_url);
						status_failure(
							&github_bot,
							&owner,
							&repo_name,
							number,
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
                            number,
                            "Auto-merge failed due to network error; see logs for details.",
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
						number,
						"Checks were unsuccessful; cancelling merge.",
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
				// Clean db.
				let _ = db.delete(commit_sha.as_bytes()).map_err(|e| {
					log::error!("Error deleting merge request from db: {}", e);
				});
			} else {
				log::info!("{} checks incomplete", html_url);
			}
		}
		Err(e) => {
			log::error!("Error getting check runs: {}", e);
		}
	}
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
	let core_devs = github_bot
		.team(owner, "core-devs")
		.and_then(|team| github_bot.team_members(team.id))
		.await
		.unwrap_or_else(|e| {
			log::error!("Error getting core devs: {}", e);
			vec![]
		});
	let team_leads = github_bot
		.team(owner, "substrateteamleads")
		.and_then(|team| github_bot.team_members(team.id))
		.await
		.unwrap_or_else(|e| {
			log::error!("Error getting core devs: {}", e);
			vec![]
		});
	let reviews = github_bot.reviews(&pr.url).await.unwrap_or_else(|e| {
		log::error!("Error getting reviews: {}", e);
		vec![]
	});

	// If merge fails then do not delete db entry.
	let mut tidy = true;

	if team_leads.iter().any(|lead| lead.login == requested_by) {
		//
		// MERGE
		//
		log::info!("{} merge requested by a team lead; merging.", pr.html_url);
		tidy = merge(github_bot, owner, repo_name, pr).await;
	//
	//
	//
	} else {
		match process::get_process(github_bot, owner, repo_name, pr.number)
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
						&format!(
							"Error getting process info; see logs for details."
						),
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
			Ok(process) => {
				let mergeable = pr.mergeable.unwrap_or(false);
				if mergeable {
					log::info!("{} is mergeable.", pr.html_url);

					let core_approved = reviews
						.iter()
						.filter(|r| {
							core_devs.iter().any(|u| u.login == r.user.login)
								&& r.state == Some(ReviewState::Approved)
						})
						.count() >= bot_config.min_reviewers;

					let owner_approved = reviews
						.iter()
						.sorted_by_key(|r| r.submitted_at)
						.rev()
						.find(|r| process.is_owner(&r.user.login))
						.map_or(false, |r| {
							r.state == Some(ReviewState::Approved)
						});

					let owner_requested = process.is_owner(&requested_by);

					let lead_approved = team_leads.iter().any(|lead| {
						reviews
							.iter()
							.sorted_by_key(|r| r.submitted_at)
							.rev()
							.find(|r| lead.login == r.user.login)
							.map_or(false, |r| {
								r.state == Some(ReviewState::Approved)
							})
					});

					if core_approved
						|| owner_approved || owner_requested
						|| lead_approved
					{
						//
						// MERGE
						//
						log::info!("{} has approval; merging.", pr.html_url);
						tidy = merge(github_bot, owner, repo_name, pr).await;
					//
					//
					//
					} else {
						if process.is_empty() {
							log::info!("{} lacks process info - it might not belong to a valid project column.", pr.html_url);
							let _ = github_bot
                                .create_issue_comment(
                                    owner,
                                    repo_name,
                                    pr.number,
                                    "PR lacks process info - check that it belongs to a valid project column.",
                                )
                                .await
                                .map_err(|e| {
                                    log::error!("Error posting comment: {}", e);
                                });
						} else {
							log::info!("{} lacks approval from the project owner or at least {} core developers.", pr.html_url, bot_config.min_reviewers);
							let _ = github_bot
                                .create_issue_comment(
                                    owner,
                                    repo_name,
                                    pr.number,
                                    &format!("PR lacks approval from the project owner or at least {} core developers.", bot_config.min_reviewers),
                                )
                                .await
                                .map_err(|e| {
                                    log::error!("Error posting comment: {}", e);
                                });
						}
					}
				} else {
					log::info!("{} is unmergeable.", pr.html_url);
					let _ = github_bot
						.create_issue_comment(
							owner,
							repo_name,
							pr.number,
							"PR is currently unmergeable.",
						)
						.await
						.map_err(|e| {
							log::error!("Error posting comment: {}", e);
						});
				}
			}
		}
	}

	// If merge was successful then delete db entry.
	if tidy {
		// Clean db.
		let _ = db.delete(pr.head.sha.as_bytes()).map_err(|e| {
			log::error!("Error deleting from db: {}", e);
		});
	}
}

/// Attempt merge and return `true` if successful, otherwise `false`.
async fn merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
) -> bool {
	if let Err(e) = github_bot
		.merge_pull_request(owner, repo_name, pr.number, &pr.head.sha)
		.await
	{
		log::error!("Error merging: {}", &e);
		// status can be false green if checks haven't fully begun.
		// in that case ignore the 405.
		let should_post = match e {
			Error::Http { source: re, .. } => {
				re.status().map_or(true, |s| s.as_u16() != 405)
			}
			_ => true,
		};
		if should_post {
			let _ = github_bot
				.create_issue_comment(
					owner,
					repo_name,
					pr.number,
					"Error merging; see logs for details.",
				)
				.await
				.map_err(|e| {
					log::error!("Error posting comment: {}", e);
				});
		}
		false
	} else {
		log::info!("Merge successful.");
		true
	}
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
			"Status failure; auto-merge cancelled.",
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
