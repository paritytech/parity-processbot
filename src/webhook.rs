use actix_web::{
	error::*, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures::{Future, Stream, StreamExt};
use futures_util::future::TryFutureExt;
use parking_lot::RwLock;
use ring::{digest, hmac, rand};
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use crate::{
	auto_merge::*,
	bamboo, bots,
	config::{BotConfig, MainConfig},
	constants::*,
	error,
	github::*,
	github_bot, matrix_bot, process,
};

pub const BAMBOO_DATA_KEY: &str = "BAMBOO_DATA";
pub const CORE_DEVS_KEY: &str = "CORE_DEVS";

pub struct AppState {
	pub db: Arc<RwLock<DB>>,
	pub github_bot: github_bot::GithubBot,
	pub matrix_bot: matrix_bot::MatrixBot,
	pub config: BotConfig,
	pub webhook_secret: String,
}

#[derive(Serialize, Deserialize)]
pub struct MergeRequest {
	sha: String,
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
		state.get_ref().webhook_secret.as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.map_err(ErrorBadRequest)?;

	let payload = serde_json::from_slice::<Payload>(&msg_bytes)
		.map_err(ErrorBadRequest)?;

	let db = &state.get_ref().db;
	let github_bot = &state.get_ref().github_bot;
	let config = &state.get_ref().config;
	match payload {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			issue:
				Issue {
					number,
					pull_request: Some(pr),
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
			log::info!("Received issue comment {} from user {}", body, login);
			if let Some(repo_name) =
				repo_url.rsplit('/').next().map(|s| s.to_string())
			{
				if let Ok(pr) =
					github_bot.pull_request(&repo_name, number).await
				{
					if body.to_lowercase().trim()
						== AUTO_MERGE_REQUEST.to_lowercase().trim()
					{
						log::info!("merge requested");
						match github_bot
							.status(&repo_name, &pr.head.sha)
							.await
							.map(|s| s.state)
						{
							Ok(StatusState::Success) => {
								let core_devs: Vec<String> = db
									.read()
									.get(CORE_DEVS_KEY.as_bytes())
									.ok()
									.flatten()
									.map(|b| {
										bincode::deserialize(&b)
											.expect("bincode deserialize")
									})
									.unwrap_or(vec![]);
								let reviews = github_bot
									.reviews(&pr)
									.await
									.unwrap_or(vec![]);
								if let Some(process) = process::get_process(
									github_bot, &repo_name, number,
								)
								.await
								{
									//									auto_merge_if_approved(
									//										github_bot, config, &core_devs,
									//										&repo_name, &pr, &process, &reviews,
									//										&login,
									//									)
									//									.await;
								}
							}
							Ok(StatusState::Pending) => {
								db.write()
									.put(
										pr.head.ref_field.as_bytes(),
										bincode::serialize(&MergeRequest {
											sha: pr.head.sha,
										})
										.expect("bincode serialize"),
									)
									.expect("db write");
							}
							Ok(StatusState::Failure) => {
								// TODO post comment
							}
							Ok(StatusState::Error) => {
								// TODO post comment
							}
							Err(e) => {
								// TODO post comment
							}
						}

						/*
						let s = String::new();
						if let Ok((reviews, issues, status)) = futures::try_join!(
							github_bot.reviews(&pr),
							github_bot.linked_issues(
								&repo_name,
								pr.body.as_ref().unwrap_or(&s)
							),
							github_bot.status(&repo_name, &pr),
						) {
							let issue_numbers = std::iter::once(pr.number)
								.chain(issues.iter().map(|issue| issue.number))
								.collect::<Vec<i64>>();
						}
						*/
					}
				}
			}
		}
		Payload::CommitStatus {
			state, branches, ..
		} => {
			log::info!("Received commit status {:?}", state);
			for Branch {
				name,
				commit: BranchCommit { sha, .. },
				..
			} in &branches
			{
				match db.read().get(name.as_bytes()) {
					Ok(Some(b)) => {
						let MergeRequest { sha: head_sha } =
							bincode::deserialize(&b)
								.expect("bincode deserialize");
						log::info!(
							"commit branch matches request {} {} {}",
							name,
							sha,
							head_sha
						)
					}
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
