use actix_web::{error::*, post, web, HttpResponse, Responder};
use futures::StreamExt;
use itertools::Itertools;
use parking_lot::RwLock;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
	config::BotConfig, constants::*, github::*, github_bot::GithubBot, process,
};

pub const BAMBOO_DATA_KEY: &str = "BAMBOO_DATA";
pub const CORE_DEVS_KEY: &str = "CORE_DEVS";

pub struct AppState {
	pub db: Arc<RwLock<DB>>,
	pub github_bot: GithubBot,
	//	pub matrix_bot: MatrixBot,
	pub bot_config: BotConfig,
	pub webhook_secret: String,
	pub environment: String,
	pub test_repo: String,
}

#[derive(Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	sha: String,
	repo_name: String,
	number: i64,
	html_url: String,
	requested_by: String,
}

#[post("/webhook")]
pub async fn webhook(
	req: web::HttpRequest,
	mut body: web::Payload,
	state: web::Data<Arc<AppState>>,
) -> actix_web::Result<impl Responder> {
	log::info!("{:?}", req);

	let mut msg_bytes = web::BytesMut::new();
	while let Some(item) = body.next().await {
		msg_bytes.extend_from_slice(&item?);
	}
	log::info!("{:?}", String::from_utf8(msg_bytes.to_vec()));

	let sig = req
		.headers()
		.get("x-hub-signature")
		.ok_or(ParseError::Incomplete)?
		.to_str()
		.map_err(ErrorBadRequest)?
		.replace("sha1=", "");
	let sig_bytes = base16::decode(sig.as_bytes()).map_err(ErrorBadRequest)?;

	verify(
		state.get_ref().webhook_secret.as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.map_err(ErrorBadRequest)?;

	let payload = serde_json::from_slice::<Payload>(&msg_bytes)
		.map_err(ErrorBadRequest)?;
	log::info!("Valid payload {:?}", payload);

	let db = &state.get_ref().db;
	let github_bot = &state.get_ref().github_bot;
	let bot_config = &state.get_ref().bot_config;
	let environment = &state.get_ref().environment;
	let test_repo = &state.get_ref().test_repo;

	match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			issue:
				Issue {
					number,
					repository_url: Some(repo_url),
					..
				},
			comment:
				Comment {
					body,
					user: User { login, .. },
					..
				},
		} => {
			if let Some(repo_name) =
				repo_url.rsplit('/').next().map(|s| s.to_string())
			{
				match github_bot.pull_request(&repo_name, number).await {
					Ok(pr) => {
						if body.to_lowercase().trim()
							== AUTO_MERGE_REQUEST.to_lowercase().trim()
						{
							log::info!(
								"Received merge request for PR {} from user {}",
								pr.html_url,
								login
							);
							match github_bot
								.status(&repo_name, &pr.head.sha)
								.await
								.map(|s| s.state)
							{
								Ok(StatusState::Success) => {
									try_merge(
										&github_bot,
										&repo_name,
										&pr,
										db,
										&bot_config,
										&login,
										&environment,
										&test_repo,
									)
									.await
								}
								Ok(StatusState::Pending) => {
									match bincode::serialize(&MergeRequest {
										sha: pr.head.sha.clone(),
										repo_name: repo_name.clone(),
										number: pr.number,
										html_url: pr.html_url.clone(),
										requested_by: login,
									}) {
										Ok(m) => match db.write().put(
											pr.head.ref_field.as_bytes(),
											m,
										) {
											Ok(_) => {
												log::info!("Auto-merge pending for PR {}", pr.html_url);
												if environment == "production"
													|| &repo_name == test_repo
												{
													let _ = github_bot
                                                    .create_issue_comment(
                                                        &repo_name,
                                                        pr.number,
                                                        &format!("Waiting for commit status."),
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
												log::error!("Error adding merge request to db: {}", e);
												if environment == "production"
													|| &repo_name == test_repo
												{
													let _ = github_bot.create_issue_comment(
                                                    &repo_name,
                                                    pr.number,
                                                    &format!("Auto-merge failed due to db error; see logs for details."),
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
										},
										Err(e) => {
											log::error!("Error serializing merge request: {}", e);
											if environment == "production"
												|| &repo_name == test_repo
											{
												let _ = github_bot.create_issue_comment(
                                                &repo_name,
                                                pr.number,
                                                &format!("Auto-merge failed due to serialization error; see logs for details."),
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
								Ok(StatusState::Failure)
								| Ok(StatusState::Error) => {
									status_failure(
										&github_bot,
										&repo_name,
										pr.number,
										&pr.html_url,
										&pr.head.ref_field,
										db,
										&environment,
										&test_repo,
									)
									.await
								}
								Err(e) => {
									log::error!(
										"Error getting PR status: {}",
										e
									);
									// Notify people of merge failure.
									if environment == "production"
										|| &repo_name == test_repo
									{
										let _ = github_bot.create_issue_comment(
                                        &repo_name,
                                        pr.number,
                                        &format!("Auto-merge failed due to network error; see logs for details."),
                                    )
                                    .await
                                    .map_err(|e| {
                                        log::error!(
                                            "Error posting comment: {}",
                                            e
                                        );
                                    });
									}
									// Clean db.
									let _ = db.write().delete(
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
					}
					Err(e) => {
						log::error!("Error getting PR: {}", e);
					}
				}
			} else {
				log::warn!("Failed parsing repo name in url: {}", repo_url);
			}
		}
		Payload::CommitStatus {
			state, branches, ..
		} => {
			for Branch {
				name: head_ref,
				commit: BranchCommit { sha, .. },
				..
			} in &branches
			{
				match db.read().get(head_ref.as_bytes()) {
					Ok(Some(b)) => match bincode::deserialize(&b) {
						Ok(MergeRequest {
							sha: head_sha,
							repo_name,
							number,
							html_url,
							requested_by,
						}) => match state {
							StatusState::Success => {
								// Head sha of branch should not have changed since request was
								// made.
								if sha == &head_sha {
									match github_bot
										.pull_request(&repo_name, number)
										.await
									{
										Ok(pr) => {
											log::info!("Commit {} on branch '{}' in repo '{}' is green; attempting merge.", sha, head_ref, repo_name);
											try_merge(
												&github_bot,
												&repo_name,
												&pr,
												db,
												&bot_config,
												&requested_by,
												environment,
												test_repo,
											)
											.await
										}
										Err(e) => {
											log::error!(
												"Error getting PR: {}",
												e
											);
											if environment == "production"
												|| &repo_name == test_repo
											{
												// Notify people of merge failure.
												let _ = github_bot.create_issue_comment(
                                                &repo_name,
                                                number,
                                                &format!("Auto-merge failed due to network error; see logs for details."),
                                            )
                                            .await
                                            .map_err(|e| {
                                                log::error!(
                                                    "Error posting comment: {}",
                                                    e
                                                );
                                            });
											}
											// Clean db.
											let _ = db.write().delete(
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
									log::warn!(
                                        "Head sha has changed since merge was requested on {}", html_url
                                    );
									if environment == "production"
										|| &repo_name == test_repo
									{
										// Notify people of merge failure.
										let _ = github_bot.create_issue_comment(
                                                &repo_name,
                                                number,
                                                &format!("Head SHA has changed since merge was requested; cancelling."),
                                            )
                                            .await
                                            .map_err(|e| {
                                                log::error!(
                                                    "Error posting comment: {}",
                                                    e
                                                );
                                            });
									}
									// Clean db.
									let _ = db.write().delete(
                                                head_ref.as_bytes(),
                                            ).map_err(|e| {
                                                log::error!(
                                                    "Error deleting merge request from db: {}",
                                                    e
                                                );
                                            });
								}
							}
							StatusState::Failure | StatusState::Error => {
								log::info!("Commit {} on branch '{}' in repo '{}' failed status checks.", sha, head_ref, repo_name);
								status_failure(
									&github_bot,
									&repo_name,
									number,
									&html_url,
									&head_ref,
									db,
									environment,
									test_repo,
								)
								.await
							}
							_ => {}
						},
						Err(e) => {
							log::error!(
								"Error deserializing merge request: {}",
								e
							);
						}
					},
					_ => {}
				}
			}
		}
		event => {
			log::info!("Received unknown event {:?}", event);
		}
	}
	Ok(HttpResponse::Ok())
}

fn verify(
	secret: &[u8],
	msg: &[u8],
	signature: &[u8],
) -> Result<(), ring::error::Unspecified> {
	let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
	log::info!("test signature {:?}", hmac::sign(&key, "testing 123".as_bytes()));
	let signed = hmac::sign(&key, msg);
	log::info!("{:?}", signed);
	hmac::verify(&key, msg, signature)
}

async fn try_merge(
	github_bot: &GithubBot,
	repo_name: &str,
	pr: &PullRequest,
	db: &Arc<RwLock<DB>>,
	bot_config: &BotConfig,
	requested_by: &str,
	environment: &str,
	test_repo: &str,
) {
	let core_devs_bytes: Vec<u8> = db
		.read()
		.get(CORE_DEVS_KEY.as_bytes())
		.unwrap_or_else(|e| {
			log::error!("Error getting core devs from db: {}", e);
			None
		})
		.unwrap_or(vec![]);
	let core_devs: Vec<String> = bincode::deserialize(&core_devs_bytes)
		.unwrap_or_else(|e| {
			log::error!("Error deserializing core devs: {}", e);
			vec![]
		});
	let reviews = github_bot.reviews(&pr.url).await.unwrap_or_else(|e| {
		log::error!("Error getting reviews: {}", e);
		vec![]
	});
	match process::get_process(github_bot, &repo_name, pr.number).await {
		Err(e) => {
			log::error!("Error getting process info: {}", e);
			if environment == "production" || repo_name == test_repo {
				// Without process info the merge cannot complete so
				// let people know.
				let _ = github_bot
					.create_issue_comment(
						&repo_name,
						pr.number,
						&format!("Error getting process info: {}", e),
					)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
		}
		Ok(process) => {
			let mergeable = pr.mergeable.unwrap_or(false);
			if mergeable {
				log::info!("{} is mergeable.", pr.html_url);

				let core_approved = reviews
					.iter()
					.filter(|r| {
						core_devs.iter().any(|u| u == &r.user.login)
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
					if environment == "production" || repo_name == test_repo {
						let _ = github_bot
							.merge_pull_request(
								&repo_name,
								pr.number,
								&pr.head.sha,
							)
							.await
							.map_err(|e| {
								log::error!("Error merging: {}", e);
							});
					}
				} else {
					if process.is_empty() {
						log::info!("{} lacks process info - it might not belong to a valid project column.", pr.html_url);
						if environment == "production" || repo_name == test_repo
						{
							let _ = github_bot
                                .create_issue_comment(
                                    &repo_name,
                                    pr.number,
                                    &format!("PR lacks process info - check that it belongs to a valid project column."),
                                )
                                .await
                                .map_err(|e| {
                                    log::error!("Error posting comment: {}", e);
                                });
						}
					} else {
						log::info!("{} lacks approval from the project owner or at least {} core developers.", pr.html_url, bot_config.min_reviewers);
						if environment == "production" || repo_name == test_repo
						{
							let _ = github_bot
                                .create_issue_comment(
                                    &repo_name,
                                    pr.number,
                                    &format!("PR lacks approval from the project owner or at least {} core developers.", bot_config.min_reviewers),
                                )
                                .await
                                .map_err(|e| {
                                    log::error!("Error posting comment: {}", e);
                                });
						}
					}
				}
			} else {
				log::info!("{} is unmergeable.", pr.html_url);
				if environment == "production" || repo_name == test_repo {
					let _ = github_bot
						.create_issue_comment(
							&repo_name,
							pr.number,
							&format!("PR is currently unmergeable."),
						)
						.await
						.map_err(|e| {
							log::error!("Error posting comment: {}", e);
						});
				}
			}
		}
	}
	// Clean db.
	let _ = db
		.write()
		.delete(pr.head.ref_field.as_bytes())
		.map_err(|e| {
			log::error!("Error deleting from db: {}", e);
		});
}

async fn status_failure(
	github_bot: &GithubBot,
	repo_name: &str,
	number: i64,
	html_url: &str,
	head_ref: &str,
	db: &Arc<RwLock<DB>>,
	environment: &str,
	test_repo: &str,
) {
	log::info!("Status failure for PR {}", html_url);
	// Notify people of merge failure.
	if environment == "production" || repo_name == test_repo {
		let _ = github_bot
			.create_issue_comment(
				&repo_name,
				number,
				&format!("Status failure; auto-merge cancelled."),
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
	}
	// Clean db.
	let _ = db.write().delete(head_ref.as_bytes()).map_err(|e| {
		log::error!("Error deleting from db: {}", e);
	});
}
