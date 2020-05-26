use actix_web::{error::*, post, web, HttpResponse, Responder};
use futures::StreamExt;
use futures_util::future::TryFutureExt;
use itertools::Itertools;
use parking_lot::RwLock;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
	config::BotConfig, constants::*, github::*, github_bot::GithubBot,
	matrix_bot::MatrixBot, process,
};

pub const BAMBOO_DATA_KEY: &str = "BAMBOO_DATA";
pub const CORE_DEVS_KEY: &str = "CORE_DEVS";

pub struct AppState {
	pub db: Arc<RwLock<DB>>,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub bot_config: BotConfig,
	pub webhook_secret: String,
	pub environment: String,
	pub test_repo: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	sha: String,
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
) -> Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	hmac::verify(&key, msg, signature)
}

#[post("/webhook")]
pub async fn webhook(
	req: web::HttpRequest,
	body: web::Payload,
	state: web::Data<Arc<AppState>>,
) -> actix_web::Result<impl Responder> {
	match handle_webhook(req, body, state).await {
		Err(e) => {
			log::error!("{:?}", e);
			Err(e)
		}
		x => x,
	}
}

async fn handle_webhook(
	req: web::HttpRequest,
	mut body: web::Payload,
	state: web::Data<Arc<AppState>>,
) -> actix_web::Result<impl Responder> {
	let mut msg_bytes = web::BytesMut::new();
	while let Some(item) = body.next().await {
		msg_bytes.extend_from_slice(&item?);
	}

	let sig = req
		.headers()
		.get("x-hub-signature")
		.ok_or(ParseError::Incomplete)?
		.to_str()
		.map_err(ErrorBadRequest)?
		.replace("sha1=", "");
	let sig_bytes = base16::decode(sig.as_bytes()).map_err(ErrorBadRequest)?;

	verify(
		state.get_ref().webhook_secret.trim().as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.map_err(ErrorBadRequest)?;

	let payload = serde_json::from_slice::<Payload>(&msg_bytes)
		.map_err(ErrorBadRequest)?;

	let db = &state.get_ref().db.write();
	let github_bot = &state.get_ref().github_bot;
	let bot_config = &state.get_ref().bot_config;

	match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			issue:
				Issue {
					number,
					html_url,
					repository_url: Some(repo_url),
					pull_request: Some(_), // indicates the issue is a pr
					..
				},
			comment:
				Comment {
					body,
					user: User { login, .. },
					..
				},
		} => {
			if let Some(owner) = GithubBot::owner_from_html_url(&html_url) {
				if let Some(repo_name) =
					repo_url.rsplit('/').next().map(|s| s.to_string())
				{
					if body.to_lowercase().trim()
						== AUTO_MERGE_REQUEST.to_lowercase().trim()
					{
						log::info!(
							"Received merge request for PR {} from user {}",
							html_url,
							login
						);
						// Fetch the pr to get all fields (eg. mergeable).
						match github_bot
							.pull_request(owner, &repo_name, number)
							.await
						{
							Ok(pr) => {
								match github_bot
									.status(owner, &repo_name, &pr.head.sha)
									.await
									.map(|s| s.state)
								{
									Ok(StatusState::Success) => {
										log::info!(
											"{} is green; attempting merge.",
											html_url
										);
										try_merge(
											&github_bot,
											owner,
											&repo_name,
											&pr,
											db,
											&bot_config,
											&login,
										)
										.await
									}
									Ok(StatusState::Pending) => {
										log::info!(
											"Status pending for PR {}",
											pr.html_url
										);
										let m = MergeRequest {
											sha: pr.head.sha.clone(),
											owner: owner.to_string(),
											repo_name: repo_name.clone(),
											number: pr.number,
											html_url: pr.html_url.clone(),
											requested_by: login,
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
														.ref_field
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
									Ok(StatusState::Failure)
									| Ok(StatusState::Error) => {
										log::info!(
											"{} failed status checks.",
											html_url
										);
										status_failure(
											&github_bot,
											owner,
											&repo_name,
											pr.number,
											&pr.html_url,
											&pr.head.ref_field,
											db,
										)
										.await
									}
									Err(e) => {
										log::error!(
											"Error getting PR status: {}",
											e
										);
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
										let _ = db.delete(
                                            pr.head.ref_field.as_bytes(),
                                        ).map_err(|e| {
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
						match github_bot
							.pull_request(owner, &repo_name, number)
							.await
						{
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
										log::error!(
											"Error posting comment: {}",
											e
										);
									});
								// Clean db.
								let _ = db.delete(
                                    pr.head.ref_field.as_bytes(),
                                ).map_err(|e| {
                                    log::error!(
                                        "Error deleting merge request from db: {}",
                                        e
                                    );
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
						match github_bot
							.pull_request(owner, &repo_name, number)
							.await
						{
							Ok(pr) => {
								match github_bot
									.latest_release(owner, &repo_name)
									.await
								{
									Ok(rel) => {
										match github_bot
											.tag(
												owner,
												&repo_name,
												&rel.tag_name,
											)
											.await
										{
											Ok(release_tag) => {
												let release_substrate_commit =
													github_bot
														.substrate_commit_from_polkadot_commit(
															&release_tag
																.object
																.sha,
														)
														.await;
												let branch_substrate_commit =
													github_bot
														.substrate_commit_from_polkadot_commit(
															&pr.head.sha,
														)
														.await;
												if release_substrate_commit
													.is_ok()
													&& branch_substrate_commit
														.is_ok()
												{
													let link = github_bot.diff_url(
                                                        owner,
                                                        "substrate",
                                                        &release_substrate_commit.unwrap(),
                                                        &branch_substrate_commit.unwrap(),
                                                    );
													log::info!("Posting link to substrate diff: {}", &link);
													let _ = github_bot
                                                        .create_issue_comment(
                                                            owner,
                                                            &repo_name,
                                                            number,
                                                            &link,
                                                        )
                                                        .await
                                                        .map_err(|e| {
                                                            log::error!(
                                                                "Error posting comment: {}",
                                                                e
                                                            );
                                                        });
												} else {
													if let Err(e) =
														release_substrate_commit
													{
														log::error!("Error getting substrate commit: {}", e);
													}
													if let Err(e) =
														branch_substrate_commit
													{
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
												log::error!(
												"Error getting release tag: {}",
												e
											);
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
										log::error!(
											"Error getting latest release: {}",
											e
										);
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
				} else {
					log::error!(
						"Failed parsing repo name in url: {}",
						repo_url
					);
				}
			} else {
				log::error!("Failed parsing owner in url: {}", html_url);
			}
		}
		Payload::CommitStatus {
			sha: _commit_sha,
			branches,
			..
		} => {
			for Branch {
				name: head_ref,
				commit: BranchCommit { sha: head_sha, .. },
				..
			} in &branches
			{
				match db.get(head_ref.trim().as_bytes()) {
					Ok(Some(b)) => match bincode::deserialize(&b) {
						Ok(m) => {
							log::info!("Deserialized merge request: {:?}", m);
							let MergeRequest {
								sha,
								owner,
								repo_name,
								number,
								html_url,
								requested_by,
							} = m;
							let status = github_bot
								.status(&owner, &repo_name, &sha)
								.await;
							log::info!("{:?}", status);
							match status {
								Ok(CombinedStatus {
									state: StatusState::Success,
									..
								}) => {
									log::info!("Combined status is success");
									// Head sha of branch should not have changed since request was
									// made.
									if &sha == head_sha {
										match github_bot
											.pull_request(
												&owner, &repo_name, number,
											)
											.await
										{
											Ok(pr) => {
												log::info!("{} is green; attempting merge.", html_url);
												try_merge(
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
												log::error!(
													"Error getting PR: {}",
													e
												);
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
                                                    head_ref.as_bytes(),
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
                                                "Head SHA has changed since merge was requested; cancelling.",
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
                                                head_ref.as_bytes(),
                                            ).map_err(|e| {
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
									log::info!(
										"{} failed status checks.",
										html_url
									);
									status_failure(
										&github_bot,
										&owner,
										&repo_name,
										number,
										&html_url,
										&head_ref,
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
									log::error!(
										"Error getting combined status: {}",
										e
									);
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
                                        head_ref.as_bytes(),
                                    ).map_err(|e| {
                                        log::error!(
                                            "Error deleting merge request from db: {}",
                                            e
                                        );
                                    });
								}
							}
						}
						Err(e) => {
							log::error!(
								"Error deserializing merge request: {}",
								e
							);
						}
					},
					Ok(None) => {
						// branch not stored for merge
					}
					Err(e) => {
						log::error!(
							"Error reading from db (head ref: {}): {}",
							head_ref,
							e
						);
					}
				}
			}
		}
		Payload::CheckRun {
			action,
			check_run:
				CheckRun {
					status,
					conclusion,
					head_sha,
					..
				},
			repository: Repository {
				html_url: repo_url, ..
			},
			..
		} => {
			log::info!("CHECK RUN");
			if let Some(owner) = GithubBot::owner_from_html_url(&repo_url) {
				if let Some(repo_name) =
					repo_url.rsplit('/').next().map(|s| s.to_string())
				{
					dbg!(&action);
					dbg!(&repo_name);
					dbg!(&status);
					dbg!(&conclusion);
					dbg!(&head_sha);
					let checks = github_bot
						.check_runs(&owner, &repo_name, &head_sha)
						.await;
					log::info!("{:?}", checks);
				}
			}
		}
		event => {
			log::info!("{:?}", event);
		}
	}
	Ok(HttpResponse::Ok())
}

async fn try_merge(
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
	let reviews = github_bot.reviews(&pr.url).await.unwrap_or_else(|e| {
		log::error!("Error getting reviews: {}", e);
		vec![]
	});
	let mut tidy = true;
	match process::get_process(github_bot, owner, repo_name, pr.number).await {
		Err(e) => {
			log::error!("Error getting process info: {}", e);
			// Without process info the merge cannot complete so
			// let people know.
			let _ = github_bot
				.create_issue_comment(
					owner,
					repo_name,
					pr.number,
					&format!("Error getting process info: {}", e),
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
					.map_or(false, |r| r.state == Some(ReviewState::Approved));

				let owner_requested = process.is_owner(&requested_by);

				if core_approved || owner_approved || owner_requested {
					log::info!("{} has approval; merging.", pr.html_url);
					let _ = github_bot
						.merge_pull_request(
							owner,
							repo_name,
							pr.number,
							&pr.head.sha,
						)
						.await
						.map_err(|e| {
							log::error!("Error merging: {}", e);
							tidy = false;
						});
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
	if tidy {
		// Clean db.
		let _ = db.delete(pr.head.ref_field.as_bytes()).map_err(|e| {
			log::error!("Error deleting from db: {}", e);
		});
	}
}

async fn status_failure(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	number: i64,
	html_url: &str,
	head_ref: &str,
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
	let _ = db.delete(head_ref.as_bytes()).map_err(|e| {
		log::error!("Error deleting from db: {}", e);
	});
}
