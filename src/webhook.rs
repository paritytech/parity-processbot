use async_recursion::async_recursion;
use futures::StreamExt;
use html_escape;
use hyper::{Body, Request, Response, StatusCode};
use reqwest::Client;

use regex::RegexBuilder;
use ring::hmac;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::collections::HashSet;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::delay_for};

use crate::{
	companion::*,
	config::MainConfig,
	error::{self, Error},
	github::*,
	github_bot::GithubBot,
	gitlab,
	rebase::*,
	utils::parse_bot_comment_from_text,
	vanity_service, CommentCommand, MergeCancelOutcome, MergeCommentCommand,
	Result, Status, WEBHOOK_PARSING_ERROR_TEMPLATE,
};

pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub config: MainConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(C)]
pub struct Dependency {
	pub sha: String,
	pub owner: String,
	pub repo: String,
	pub number: i64,
	pub html_url: String,
	pub is_directly_referenced: bool,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(C)]
pub struct MergeRequest {
	pub sha: String,
	pub was_updated: bool,
	pub owner: String,
	pub repo: String,
	pub number: i64,
	pub html_url: String,
	pub requested_by: String,
	pub dependencies: Option<Vec<Dependency>>,
}

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
			.context(error::Message {
				msg: "Missing x-hub-signature".to_owned(),
			})?
			.to_str()
			.ok()
			.context(error::Message {
				msg: "Error parsing x-hub-signature".to_owned(),
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

pub async fn webhook_inner(
	mut req: Request<Body>,
	state: &AppState,
) -> Result<(MergeCancelOutcome, Result<()>)> {
	let mut msg_bytes = vec![];
	while let Some(item) = req.body_mut().next().await {
		msg_bytes.extend_from_slice(&item.ok().context(error::Message {
			msg: "Error getting bytes from request body".to_owned(),
		})?);
	}

	let sig = req
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
	let sig_bytes =
		base16::decode(sig.as_bytes())
			.ok()
			.context(error::Message {
				msg: "Error decoding x-hub-signature".to_owned(),
			})?;

	let AppState { config, .. } = state;

	verify(
		config.webhook_secret.trim().as_bytes(),
		&msg_bytes,
		&sig_bytes,
	)
	.ok()
	.context(error::Message {
		msg: "Validation signature does not match".to_owned(),
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
			.and_then(|detected| detected.get_issue_details());

			if let Some(pr_details) = pr_details {
				Err(Error::Message {
					msg: format!(
						WEBHOOK_PARSING_ERROR_TEMPLATE!(),
						err,
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
						repository_url,
						pull_request: Some(_),
					} => {
						let (sha, result) = handle_comment(
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
				StatusState::Unknown => Ok(()),
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
	};

	// From this point onwards we'll clean the SHA from the database if this is a error which stops
	// the merge process

	// Without the SHA we'll not be able to fetch the database for more context, so exit early
	let sha = match sha {
		Some(sha) => sha,
		None => return (MergeCancelOutcome::ShaNotFound, result),
	};

	// If it's not an error then don't bother with going further
	let err = match result {
		Ok(_) => return (MergeCancelOutcome::WasNotCancelled, Ok(())),
		Err(err) => err,
	};

	// If this error does not interrupt the merge process, then don't bother with going further
	if !err.stops_merge_attempt() {
		log::info!(
			"SHA {} did not have its merge attempt stopped because error does not stop the merge attempt {:?}",
			sha,
			err
		);
		return (MergeCancelOutcome::WasNotCancelled, Err(err));
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
					let merge_cancel_outcome = match cleanup_pr(
						state,
						&sha,
						&mr.owner,
						&mr.repo,
						mr.number,
						&PullRequestCleanupReason::Cancelled,
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

					(
						merge_cancel_outcome,
						Err(err.map_issue((mr.owner, mr.repo, mr.number))),
					)
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
	state: &AppState,
	owner: &str,
	repo: &str,
	commit_sha: &str,
	html_url: &str,
	should_handle_retried_jobs: bool,
) -> Result<(Status, HashMap<String, (i64, StatusState, Option<String>)>)> {
	let AppState {
		github_bot, config, ..
	} = state;

	let statuses = github_bot.status(owner, repo, commit_sha).await?;
	log::info!("{} statuses: {:?}", html_url, statuses);

	// Since Github only considers the latest instance of each status, we should
	// abide by the same rule. Each instance is uniquely identified by "context".
	let mut latest_statuses: HashMap<
		String,
		(i64, StatusState, Option<String>),
	> = HashMap::new();
	for s in statuses {
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
			.map(|(prev_id, _, _)| prev_id < &s.id)
			.unwrap_or(true)
		{
			latest_statuses.insert(s.context, (s.id, s.state, s.target_url));
		}
	}
	log::info!("{} latest_statuses: {:?}", html_url, latest_statuses);

	if latest_statuses
		.values()
		.all(|(_, state, _)| *state == StatusState::Success)
	{
		log::info!("{} has success status", html_url);
		Ok((Status::Success, latest_statuses))
	} else if latest_statuses.values().any(|(_, state, _)| {
		*state == StatusState::Error || *state == StatusState::Failure
	}) {
		if should_handle_retried_jobs {
			let mut has_failed_non_gitlab_job = false;

			let gitlab_job_target_url_matcher =
				RegexBuilder::new(r"^(\w+://[^/]+)/(.*)/builds/([0-9]+)$")
					.case_insensitive(true)
					.build()
					.unwrap();
			let failed_gitlab_jobs = latest_statuses
				.values()
				.filter_map(|(_, status, target_url)| match *status {
					StatusState::Failure | StatusState::Error => {
						let gitlab_job_data =
							target_url.as_ref().and_then(|target_url| {
								gitlab_job_target_url_matcher
									.captures(target_url)
									.and_then(|matches| {
										let gitlab_url =
											matches.get(1).unwrap().as_str();
										if gitlab_url == config.gitlab_url {
											let gitlab_project = matches
												.get(2)
												.unwrap()
												.as_str();
											let job_id = matches
												.get(3)
												.unwrap()
												.as_str()
												.parse::<usize>()
												.unwrap();
											Some((
												gitlab_url,
												gitlab_project,
												job_id,
											))
										} else {
											None
										}
									})
							});
						if gitlab_job_data.is_none() {
							has_failed_non_gitlab_job = true;
						}
						gitlab_job_data
					}
					_ => None,
				})
				.collect::<Vec<_>>();

			if !has_failed_non_gitlab_job {
				let mut recovered_jobs = vec![];

				let http_client = Client::new();
				for (gitlab_url, gitlab_project, job_id) in failed_gitlab_jobs {
					// https://docs.gitlab.com/ee/api/jobs.html#get-a-single-job
					let job_api_url = format!(
						"{}/api/v4/projects/{}/jobs/{}",
						gitlab_url,
						urlencoding::encode(gitlab_project),
						job_id
					);

					let job = http_client
						.execute(
							http_client
								.get(&job_api_url)
								.headers(gitlab::get_request_headers(
									&config.gitlab_access_token,
								)?)
								.build()
								.map_err(|err| Error::Message {
									msg: format!(
										"Failed to build request to fetch {} due to {:?}",
										job_api_url,
										err
									),
								})?,
						)
						.await
						.context(error::Http)?
						.json::<gitlab::GitlabJob>()
						.await
						.context(error::Http)?;

					log::info!("Fetched job for {}: {:?}", job_api_url, job);

					match job.pipeline.status {
						gitlab::GitlabPipelineStatus::Created
						| gitlab::GitlabPipelineStatus::WaitingForResource
						| gitlab::GitlabPipelineStatus::Preparing
						| gitlab::GitlabPipelineStatus::Pending
						| gitlab::GitlabPipelineStatus::Running
						| gitlab::GitlabPipelineStatus::Scheduled => {
							log::info!("{} is failing on GitHub, but its pipeline is pending, therefore we'll check if it's running or pending (it might have been retried)", job_api_url);

							let pending_or_successful_jobs = {
								let mut pending_or_successful_jobs = vec![];
								// https://docs.gitlab.com/ee/api/#offset-based-pagination
								let mut page = 1;
								loop {
									// https://docs.gitlab.com/ee/api/jobs.html#list-pipeline-jobs
									let pending_or_successful_jobs_api = format!(
										"{}/api/v4/projects/{}/pipelines/{}/jobs?scope[]=pending&scope[]=running&scope[]=success&scope[]=created&per_page=100&page={}",
										gitlab_url,
										job.pipeline.project_id,
										job.pipeline.id,
										page
									);

									let page_pending_or_successful_jobs = http_client
										.execute(
											http_client
												.get(&pending_or_successful_jobs_api)
												.headers(gitlab::get_request_headers(
													&config.gitlab_access_token,
												)?)
												.build()
												.map_err(|err| Error::Message {
													msg: format!(
														"Failed to build request to fetch {} due to {:?}",
														pending_or_successful_jobs_api,
														err
													),
												})?,
										)
										.await
										.context(error::Http)?
										.json::<Vec<gitlab::GitlabPipelineJob>>()
										.await
										.context(error::Http)?;

									if page_pending_or_successful_jobs
										.is_empty()
									{
										break;
									}

									pending_or_successful_jobs.extend(
										page_pending_or_successful_jobs,
									);

									page += 1;
								}
								pending_or_successful_jobs
							};

							if pending_or_successful_jobs.iter().any(
								|pending_pipeline_job| {
									pending_pipeline_job.name == job.name
								},
							) {
								recovered_jobs.push(job_api_url);
							} else {
								log::info!(
									"{} 's pipeline (id: {}) for job {} (name: {}) did not list it as pending or successful, therefore the job is considered to be failing",
									html_url,
									job.pipeline.id,
									job_api_url,
									job.name
								);
								recovered_jobs.clear();
								break;
							}
						}
						_ => {
							log::info!(
								"{} 's pipeline (id: {}) for job {} (name: {}) is not pending, therefore the job itself can't be considered to be pending",
								html_url,
								job.pipeline.id,
								job_api_url,
								job.name,
							);
							recovered_jobs.clear();
							break;
						}
					}
				}

				if !recovered_jobs.is_empty() {
					log::info!(
						"{} was initially considered to be failing, but we consider it has recovered because the following jobs have recovered: {:?}",
						html_url,
						recovered_jobs
					);
					return Ok((Status::Pending, latest_statuses));
				}
			}
		}

		log::info!("{} has failed status", html_url);
		Ok((Status::Failure, latest_statuses))
	} else {
		log::info!("{} has pending status", html_url);
		Ok((Status::Pending, latest_statuses))
	}
}

pub async fn get_latest_checks_state(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let checks = github_bot.check_runs(owner, repo_name, commit_sha).await?;
	log::info!("{} checks: {:?}", html_url, checks);

	// Since Github only considers the latest instance of each check, we should abide by the same
	// rule. Each instance is uniquely identified by "name".
	let mut latest_checks = HashMap::new();
	for c in checks.check_runs {
		if latest_checks
			.get(&c.name)
			.map(|(prev_id, _, _)| prev_id < &c.id)
			.unwrap_or(true)
		{
			latest_checks.insert(c.name, (c.id, c.status, c.conclusion));
		}
	}
	log::info!("{} latest_checks: {:?}", html_url, latest_checks);

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

/// Act on a status' outcome to decide on whether a PR relating to this SHA is ready to be merged
#[async_recursion]
pub async fn checks_and_status(state: &AppState, sha: &str) -> Result<()> {
	let AppState { db, github_bot, .. } = state;

	log::info!("Checking for statuses of {}", sha);

	let mr: MergeRequest = match db.get(sha.as_bytes()).context(error::Db)? {
		Some(bytes) => bincode::deserialize(&bytes).context(error::Bincode)?,
		None => return Ok(()),
	};
	let pr = github_bot
		.pull_request(&mr.owner, &mr.repo, mr.number)
		.await?;
	log::info!(
		"Deserialized merge request for {} (sha {}): {:?}",
		pr.html_url,
		sha,
		mr
	);

	match async {
		if handle_merged_pr(state, &pr, &mr.requested_by).await? {
			return Ok(());
		}

		if mr.sha != pr.head.sha {
			return Err(Error::HeadChanged {
				expected: sha.to_string(),
				actual: pr.head.sha.to_owned(),
			});
		}

		if !ready_to_merge(state, &pr).await? {
			log::info!("{} is not ready", pr.html_url);
			return Ok(());
		}

		check_merge_is_allowed(state, &pr, &mr.requested_by, &[]).await?;

		if let Some(dependencies) = &mr.dependencies {
			for dependency in dependencies {
				let dependency_pr = github_bot
					.pull_request(
						&dependency.owner,
						&dependency.repo,
						dependency.number,
					)
					.await?;
				if dependency_pr.head.sha != dependency.sha {
					return Err(Error::Message {
						msg: format!(
							"Dependency {} 's HEAD SHA changed from {} to {}. Aborting.",
							dependency.html_url,
							dependency.sha,
							dependency_pr.head.sha
						),
					});
				}

				if dependency_pr.merged {
					log::info!(
						"Dependency {} of PR {} was merged, cleaning it",
						dependency_pr.html_url,
						pr.html_url
					);
					cleanup_pr(
						state,
						&dependency_pr.head.sha,
						&dependency.owner,
						&dependency.repo,
						dependency.number,
						&PullRequestCleanupReason::AfterMerge,
					)
					.await?;
				} else {
					log::info!(
						"Giving up on merging {} because its dependency {} has not been merged yet",
						pr.html_url,
						dependency.html_url
					);
					return Ok(());
				};
			}
		}

		log::info!("Updating companion {} before merge", pr.html_url);
		update_then_merge(
			state,
			&mr,
			&WaitToMergeMessage::None,
			// No need to register the MR again: we already know it is registered because
			// it was fetched from the database at the start
			false,
			// We have checked that all dependencies are ready by this point
			true,
		)
		.await?;

		Ok(())
	}
	.await
	{
		Ok(_) | Err(Error::MergeFailureWillBeSolvedLater { .. }) => Ok(()),
		Err(err) => Err(err.map_issue((
			pr.base.repo.owner.login,
			pr.base.repo.name,
			pr.number,
		))),
	}
}

pub async fn handle_dependents_after_merge(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
) -> Result<()> {
	log::info!("Handling dependents of {}", pr.html_url);

	let AppState {
		github_bot,
		db,
		config,
		..
	} = state;

	let fetched_dependents = github_bot
		.resolve_pr_dependents(config, pr, requested_by, &[])
		.await?;
	if let Some(dependents) = &fetched_dependents {
		log::info!(
			"Found current dependents of {}: {:?}",
			pr.html_url,
			dependents
		);
	}

	/*
		The alive dependents are the ones which are still referenced in the PR
		description plus the ones from the database were registered as indirect
		dependencies
	*/
	let mut alive_dependents = fetched_dependents.clone().unwrap_or_default();

	// Helper function to avoid duplicate dependents from being registered
	let mut register_alive_dependent = |dep: MergeRequest| {
		if alive_dependents.iter().any(|alive_dep: &MergeRequest| {
			dep.owner == alive_dep.owner
				&& dep.repo == alive_dep.repo
				&& dep.number == alive_dep.number
		}) {
			return;
		};
		alive_dependents.push(dep)
	};

	/*
		Step 1: Update dangling references

		The dependents we have detected when the merge chain was first built might not
		be referenced in the PR description anymore (i.e. they have become dangling
		references); in that case try to invalidate them from the database

		---

		Set up a loop for reinitializing the DB's iterator since the operations
		performed in this loop might modify or delete multiple items from the
		database, thus potentially making the iteration not work according to
		expectations.
	*/
	let mut processed_mrs = vec![];
	'db_iteration_loop: loop {
		let db_iter = db.iterator(rocksdb::IteratorMode::Start);
		'to_next_item: for (key, value) in db_iter {
			match bincode::deserialize::<MergeRequest>(&value)
				.context(error::Bincode)
			{
				Ok(mut mr) => {
					if processed_mrs.iter().any(|prev_mr: &MergeRequest| {
						mr.owner == prev_mr.owner
							&& mr.repo == prev_mr.repo && mr.number
							== prev_mr.number
					}) {
						continue;
					}

					if let Some(dependents) = &fetched_dependents {
						for dependent in dependents {
							if dependent.owner == mr.owner
								&& dependent.repo == mr.repo && dependent.number
								== mr.number
							{
								// This item was detected a dependent, therefore it is not potentially
								// dangling for this PR specifically
								register_alive_dependent(mr);
								continue 'to_next_item;
							}
						}
					}

					#[derive(PartialEq)]
					enum LivenessOutcome {
						Updated,
						Dangling,
						Alive,
						AliveNeedsUpdate,
					}
					let mut liveness_outcome: Option<LivenessOutcome> = None;

					mr.dependencies = mr.dependencies.map(|dependencies| {
						dependencies
							.into_iter()
							.filter(|dependency| {
								if dependency.owner == pr.base.repo.owner.login
									&& dependency.repo == pr.base.repo.name
									&& dependency.number == pr.number
								{
									if dependency.is_directly_referenced {
										if liveness_outcome.is_none() {
											liveness_outcome =
												Some(LivenessOutcome::Dangling);
										}
										false
									} else {
										if liveness_outcome != Some(LivenessOutcome::AliveNeedsUpdate) {
											liveness_outcome = match liveness_outcome {
												Some(LivenessOutcome::Updated) => Some(LivenessOutcome::AliveNeedsUpdate),
												_ => Some(LivenessOutcome::Alive)
											};
										}
										true
									}
								} else {
									if let Some(LivenessOutcome::Dangling) =
										liveness_outcome
									{
										liveness_outcome =
											Some(LivenessOutcome::Updated);
									}
									true
								}
							})
							.collect()
					});

					if let Some(liveness_outcome) = liveness_outcome {
						match liveness_outcome {
							LivenessOutcome::Alive => {
								register_alive_dependent(mr.clone());
							}
							LivenessOutcome::Updated
							| LivenessOutcome::AliveNeedsUpdate => {
								if let Err(err) = db
									.put(
										&key,
										bincode::serialize(&mr)
											.context(error::Bincode)?,
									)
									.context(error::Db)
								{
									log::error!(
										"Failed to update database references after merge of {} in dependent {} due to {:?}",
										pr.html_url,
										mr.html_url,
										err
									);
									let _ = cleanup_pr(
										state,
										&mr.sha,
										&mr.owner,
										&mr.repo,
										mr.number,
										&PullRequestCleanupReason::Error,
									)
									.await;
									handle_error(
										MergeCancelOutcome::WasCancelled,
										Error::Message {
											msg: format!(
												"Unable to update {} in the database (detected as a dependent of {})",
												&mr.html_url,
												pr.html_url
											),
										}
										.map_issue((
											(&mr.owner).into(),
											(&mr.repo).into(),
											mr.number,
										)),
										state,
									)
									.await;
								} else if liveness_outcome
									== LivenessOutcome::AliveNeedsUpdate
								{
									register_alive_dependent(mr.clone());
								}
							}
							LivenessOutcome::Dangling => {
								let _ = db.delete(&key);
							}
						};

						processed_mrs.push(mr);
						continue 'db_iteration_loop;
					}
				}
				Err(err) => {
					log::error!(
						"Failed to deserialize key {} from the database due to {:?}",
						String::from_utf8_lossy(&key),
						err
					);
					let _ = db.delete(&key);
				}
			};
		}
		break;
	}

	let dependents = {
		if alive_dependents.is_empty() {
			return Ok(());
		}
		alive_dependents
	};

	/*
		Step 2: Update the dependents (and merge them right away if possible)

		Update dependents which can be updated (i.e. those who have the PR which was
		just merged as their *only* pending dependency)
	*/
	let mut updated_dependents: Vec<(String, &MergeRequest)> = vec![];
	for dependent in &dependents {
		let depends_on_another_pr = dependent
			.dependencies
			.as_ref()
			.map(|dependencies| {
				dependencies
					.iter()
					.any(|dependency| dependency.repo != pr.base.repo.name)
			})
			.unwrap_or(false);
		match update_then_merge(
			state,
			dependent,
			&WaitToMergeMessage::Default,
			// The dependent should always be registered to the database as a pending
			// item since one of its dependencies just got merged, therefore it becomes
			// eligible for merge in the future
			true,
			!depends_on_another_pr,
		)
		.await
		{
			Ok(updated_sha) => {
				if let Some(updated_sha) = updated_sha {
					updated_dependents.push((updated_sha, dependent))
				}
			}
			Err(err) => {
				let _ = cleanup_pr(
					state,
					&dependent.sha,
					&dependent.owner,
					&dependent.repo,
					dependent.number,
					&PullRequestCleanupReason::Error,
				)
				.await;
				handle_error(
					MergeCancelOutcome::WasCancelled,
					err.map_issue((
						(&dependent.owner).into(),
						(&dependent.repo).into(),
						dependent.number,
					)),
					state,
				)
				.await;
			}
		}
	}

	/*
		Step 3: Collect the relevant dependents which should be checked

		If the dependent was merged in the previous step or someone merged it manually
		in-between this step and the previous one, the dependents of that dependent
		should be collected for the check because they might be mergeable now,
		because one of its dependencies (the dependent) was merged.

		If the dependent was updated in the previous step, it might already be
		mergeable (i.e. their statuses might already be passing after the update),
		therefore it should be included in the dependents_to_check. Also, since it was
		updated, its dependencies should be updated as well to track the resulting SHA
		after the update, otherwise their processing would result in the HeadChanged
		error unintendedly (HeadChanged is a security measure to prevent malicious
		commits from sneaking in after the chain is built, but in this case we changed
		the HEAD of the PR ourselves through the update, which is safe).
	*/
	let mut dependents_to_check = HashMap::new();
	let db_iter = db.iterator(rocksdb::IteratorMode::Start);
	for (key, value) in db_iter {
		match bincode::deserialize::<MergeRequest>(&value)
			.context(error::Bincode)
		{
			Ok(mut dependent_of_dependent) => {
				let mut should_be_included_in_check = false;
				let mut record_needs_update = false;

				let mut updated_dependencies = HashSet::new();
				dependent_of_dependent.dependencies =
					if let Some(mut dependencies) =
						dependent_of_dependent.dependencies
					{
						for dependency in dependencies.iter_mut() {
							for (updated_sha, updated_dependent) in
								&updated_dependents
							{
								if dependency.owner == updated_dependent.owner
									&& dependency.repo == updated_dependent.repo
									&& dependency.number
										== updated_dependent.number
								{
									record_needs_update = true;
									log::info!(
										"Updating {} 's dependency on {} to SHA {}",
										dependency.html_url,
										dependent_of_dependent.html_url,
										updated_sha,
									);
									dependency.sha = updated_sha.clone();
									updated_dependencies
										.insert(&updated_dependent.html_url);
								}
							}
							if dependency.owner == pr.base.repo.owner.login
								&& dependency.repo == pr.base.repo.name
								&& dependency.number == pr.number
							{
								should_be_included_in_check = true;
							}
						}
						Some(dependencies)
					} else {
						None
					};

				if record_needs_update {
					if let Err(err) = db
						.put(
							&key,
							bincode::serialize(&dependent_of_dependent)
								.context(error::Bincode)?,
						)
						.context(error::Db)
					{
						log::error!(
							"Failed to update a dependent to {:?} due to {:?}",
							dependent_of_dependent,
							err
						);
						let _ = cleanup_pr(
							state,
							&dependent_of_dependent.sha,
							&dependent_of_dependent.owner,
							&dependent_of_dependent.repo,
							dependent_of_dependent.number,
							&PullRequestCleanupReason::Error,
						)
						.await;
						handle_error(
							MergeCancelOutcome::WasCancelled,
							Error::Message {
								msg: format!(
									 "Failed to update database references of {:?} in dependent {} after the merge of {}",
									 updated_dependencies,
									 dependent_of_dependent.html_url,
									 pr.html_url
								),
							}
							.map_issue((
								(&dependent_of_dependent.owner).into(),
								(&dependent_of_dependent.repo).into(),
								dependent_of_dependent.number,
							)),
							state,
						)
						.await;
					} else {
						dependents_to_check.insert(key, dependent_of_dependent);
					}
				} else if should_be_included_in_check {
					dependents_to_check.insert(key, dependent_of_dependent);
				}
			}
			Err(err) => {
				log::error!(
					"Failed to deserialize key {} from the database due to {:?}",
					String::from_utf8_lossy(&key),
					err
				);
				let _ = db.delete(&key);
			}
		};
	}

	/*
		Step 4: Check the dependents collected in the previous step

		Because the PR passed as an argument to this function is merged and its
		dependents might have been merged in the previous steps, the dependents we
		collected (which might include dependents of the dependents which were just
		merged) might have become ready to be merged at this point.
	*/
	for dependent in dependents_to_check.into_values() {
		if let Err(err) = checks_and_status(state, &dependent.sha).await {
			let _ = cleanup_pr(
				state,
				&dependent.sha,
				&dependent.owner,
				&dependent.repo,
				dependent.number,
				&PullRequestCleanupReason::Error,
			)
			.await;
			handle_error(MergeCancelOutcome::WasCancelled, err, state).await;
		}
	}

	Ok(())
}

async fn handle_command(
	state: &AppState,
	cmd: &CommentCommand,
	pr: &PullRequest,
	requested_by: &str,
) -> Result<()> {
	let AppState { github_bot, .. } = state;

	match cmd {
		// This command marks the start of the chain of merges. The PR where the
		// command was received will act as the starting point for resolving further
		// dependencies.
		CommentCommand::Merge(cmd) => {
			let mr = MergeRequest {
				sha: (&pr.head.sha).into(),
				owner: (&pr.base.repo.owner.login).into(),
				repo: (&pr.base.repo.name).into(),
				number: pr.number,
				html_url: (&pr.html_url).into(),
				requested_by: requested_by.into(),
				// Set "was_updated" from the start so that this branch will not be updated
				// It's important for it not to be updated because the command issuer has
				// trusted the current commit, but not the ones coming after it (some
				// malicious actor might want to sneak in changes after the command starts).
				was_updated: true,
				// This is the starting point of the merge chain, hence why always no
				// dependencies are registered for it upfront
				dependencies: None,
			};

			check_merge_is_allowed(state, pr, requested_by, &[]).await?;

			match cmd {
				MergeCommentCommand::Normal => {
					if ready_to_merge(state, pr).await? {
						match merge(state, pr, requested_by).await? {
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
									&mr,
									&WaitToMergeMessage::Custom(&msg),
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
						wait_to_merge(state, &mr, &WaitToMergeMessage::Default)
							.await?;
						return Ok(());
					}
				}
				MergeCommentCommand::Force => {
					match merge(state, pr, requested_by).await? {
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

			handle_dependents_after_merge(state, pr, requested_by).await
		}
		CommentCommand::CancelMerge => {
			log::info!("Deleting merge request for {}", pr.html_url);

			cleanup_pr(
				state,
				&pr.head.sha,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
				&PullRequestCleanupReason::Cancelled,
			)
			.await?;

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
		let owner = owner_from_html_url(html_url).context(error::Message {
			msg: format!("Failed parsing owner in url: {}", html_url),
		})?;

		let repo = repo_url.rsplit('/').next().context(error::Message {
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
			delay_for(Duration::from_millis(config.merge_command_delay)).await;
		};

		let pr = github_bot.pull_request(owner, repo, number).await?;

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

pub async fn check_merge_is_allowed(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
	companion_reference_trail: &[CompanionReferenceTrailItem],
) -> Result<()> {
	if !pr.mergeable.unwrap_or(false) {
		return Err(Error::Message {
			msg: format!("Github API says {} is not mergeable", pr.html_url),
		});
	} else {
		log::info!("{} is mergeable", pr.html_url);
	}

	return check_all_companions_are_mergeable(
		state,
		pr,
		requested_by,
		companion_reference_trail,
	)
	.await;
}

pub async fn ready_to_merge(
	state: &AppState,
	pr: &PullRequest,
) -> Result<bool> {
	let AppState { github_bot, .. } = state;

	match get_latest_checks_state(
		github_bot,
		&pr.base.repo.owner.login,
		&pr.base.repo.name,
		&pr.head.sha,
		&pr.html_url,
	)
	.await?
	{
		Status::Success => {
			match get_latest_statuses_state(
				state,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				&pr.head.sha,
				&pr.html_url,
				true,
			)
			.await?
			.0
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

async fn register_merge_request(
	state: &AppState,
	mr: &MergeRequest,
) -> Result<()> {
	let AppState { db, .. } = state;
	let MergeRequest { sha, .. } = mr;
	log::info!("Registering merge request (sha: {}): {:?}", sha, mr);
	db.put(
		sha.as_bytes(),
		bincode::serialize(mr).context(error::Bincode)?,
	)
	.context(error::Db)
}

pub enum WaitToMergeMessage<'a> {
	Custom(&'a str),
	Default,
	None,
}
/// Create a merge request, add it to the database, and post a comment stating the merge is
/// pending.
pub async fn wait_to_merge(
	state: &AppState,
	mr: &MergeRequest,
	msg: &WaitToMergeMessage<'_>,
) -> Result<()> {
	register_merge_request(state, mr).await?;

	let AppState { github_bot, .. } = state;

	let MergeRequest {
		owner,
		repo,
		number,
		..
	} = mr;

	let msg = match msg {
		WaitToMergeMessage::Custom(msg) => msg,
		WaitToMergeMessage::Default => "Waiting for commit status.",
		WaitToMergeMessage::None => return Ok(()),
	};

	let post_comment_result = github_bot
		.create_issue_comment(owner, repo, *number, msg)
		.await;
	if let Err(err) = post_comment_result {
		log::error!("Error posting comment: {}", err);
	}

	Ok(())
}

pub async fn handle_merged_pr(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
) -> Result<bool> {
	if !pr.merged {
		return Ok(false);
	}

	let was_cleaned_up = cleanup_pr(
		state,
		&pr.head.sha,
		&pr.base.repo.owner.login,
		&pr.base.repo.name,
		pr.number,
		&PullRequestCleanupReason::AfterMerge,
	)
	.await
	.map(|_| true);

	/*
		It's not sane to try to handle the dependents if the cleanup went wrong since
		that hints at some bug in the application
	*/
	if was_cleaned_up.is_ok() {
		if let Err(err) =
			handle_dependents_after_merge(state, pr, requested_by).await
		{
			log::error!(
				"Failed to process handle_dependents_after_merge in cleanup_merged_pr due to {:?}",
				err
			);
		}
	}

	was_cleaned_up
}

pub enum PullRequestCleanupReason<'a> {
	AfterMerge,
	AfterSHAUpdate(&'a String),
	Cancelled,
	Error,
}

// Removes a pull request from the database (e.g. when it has been merged) and
// executes side-effects related to the kind of trigger for this function
pub async fn cleanup_pr(
	state: &AppState,
	key_to_guarantee_deleted: &str,
	owner: &str,
	repo: &str,
	number: i64,
	reason: &PullRequestCleanupReason<'_>,
) -> Result<()> {
	let AppState { db, .. } = state;

	let mut related_dependents = HashMap::new();

	let db_iter = db.iterator(rocksdb::IteratorMode::Start);
	'to_next_db_item: for (key, value) in db_iter {
		match bincode::deserialize::<MergeRequest>(&value)
			.context(error::Bincode)
		{
			Ok(mr) => {
				if mr.owner == owner && mr.repo == repo && mr.number == number {
					log::info!(
						"Cleaning up {:?} due to key {} of {}/{}/pull/{}",
						mr,
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

				if let Some(dependencies) = &mr.dependencies {
					for dependency in dependencies.iter() {
						if dependency.owner == owner
							&& dependency.repo == repo && dependency.number
							== number
						{
							related_dependents.insert((&mr.sha).clone(), mr);
							continue 'to_next_db_item;
						}
					}
				}
			}
			Err(err) => {
				log::error!(
					"Failed to deserialize key {} from the database due to {:?}",
					String::from_utf8_lossy(&key),
					err
				);
			}
		}
	}

	// Sanity check: the key should have actually been deleted
	if db
		.get(key_to_guarantee_deleted)
		.context(error::Db)?
		.is_some()
	{
		return Err(Error::Message {
			msg: format!(
				"Key {} was not deleted from the database",
				key_to_guarantee_deleted
			),
		});
	}

	struct CleanedUpPullRequest {
		pub owner: String,
		pub repo: String,
		pub key_to_guarantee_deleted: String,
		pub number: i64,
	}
	lazy_static::lazy_static! {
		static ref CLEANUP_PR_RECURSION_PREVENTION: parking_lot::Mutex<Vec<CleanedUpPullRequest>> = {
			parking_lot::Mutex::new(vec![])
		};
	}
	// Prevent mutual recursion since the side-effects might end up calling this
	// function again. We want to trigger the further side-effects at most once for
	// each pull request.
	{
		log::info!("Acquiring cleanup_pr's recursion prevention lock");
		let mut cleaned_up_prs = CLEANUP_PR_RECURSION_PREVENTION.lock();
		for pr in &*cleaned_up_prs {
			if pr.owner == owner
				&& pr.repo == repo
				&& pr.number == number
				&& pr.key_to_guarantee_deleted == key_to_guarantee_deleted
			{
				log::info!(
					"Skipping side-effects of {}/{}/pull/{} (key {}) because they have already been processed",
					owner,
					repo,
					number,
					key_to_guarantee_deleted
				);
				return Ok(());
			}
		}
		cleaned_up_prs.push(CleanedUpPullRequest {
			owner: owner.into(),
			repo: repo.into(),
			key_to_guarantee_deleted: key_to_guarantee_deleted.into(),
			number,
		});
		log::info!("Releasing cleanup_pr's recursion prevention lock");
	}

	log::info!(
		"Related dependents of {}/{}/pull/{} (key {}): {:?}",
		owner,
		repo,
		number,
		key_to_guarantee_deleted,
		related_dependents
	);

	match reason {
		PullRequestCleanupReason::Error
		| PullRequestCleanupReason::Cancelled => {
			for dependent in related_dependents.values() {
				let _ = cleanup_pr(
					state,
					&dependent.sha,
					&dependent.owner,
					&dependent.repo,
					dependent.number,
					reason,
				);
			}
		}
		PullRequestCleanupReason::AfterSHAUpdate(updated_sha) => {
			for mut dependent in related_dependents.into_values() {
				let mut was_updated = false;
				dependent.dependencies =
					if let Some(mut dependencies) = dependent.dependencies {
						for dependency in dependencies.iter_mut() {
							if dependency.owner == owner
								&& dependency.repo == repo && dependency.number
								== number
							{
								was_updated = true;
								log::info!(
									"Dependency of {} on {}/{}/pull/{} was updated to SHA {}",
									dependent.html_url,
									owner,
									repo,
									number,
									updated_sha
								);
								dependency.sha = updated_sha.to_string();
							}
						}
						Some(dependencies)
					} else {
						None
					};

				if was_updated {
					db.put(
						dependent.sha.as_bytes(),
						bincode::serialize(&dependent)
							.context(error::Bincode)?,
					)
					.context(error::Db)?;
				}
			}
		}
		PullRequestCleanupReason::AfterMerge => {}
	}

	log::info!("Cleaning up cleanup_pr recursion prevention lock's entries");
	CLEANUP_PR_RECURSION_PREVENTION.lock().clear();

	Ok(())
}

pub async fn merge(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
) -> Result<Result<()>> {
	if handle_merged_pr(state, pr, requested_by).await? {
		return Ok(Ok(()));
	}

	let AppState { github_bot, .. } = state;

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
			// Merge succeeded! Now clean it from the database
			if let Err(err) = cleanup_pr(
				state,
				&pr.head.sha,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
				&PullRequestCleanupReason::AfterMerge,
			)
			.await
			{
				log::error!(
					"Failed to cleanup PR on the database after merge: {}",
					err
				);
			};
			return Ok(Ok(()));
		}
		Err(err) => err,
	};

	let msg = match err {
		Error::Response {
			ref status,
			ref body,
		} if *status == StatusCode::METHOD_NOT_ALLOWED => match body.get("message") {
			Some(msg) => match msg.as_str() {
				Some(msg) => msg,
				None => {
					log::error!("Expected \"message\" of Github API merge failure response to be a string");
					return Err(err);
				}
			},
			None => {
				log::error!("Expected \"message\" of Github API merge failure response to be available");
				return Err(err);
			}
		},
		_ => return Err(err),
	};

	// Matches the following
	// - "Required status check ... is {pending,expected}."
	// - "... required status checks have not succeeded: ... {pending,expected}."
	let missing_status_matcher =
		RegexBuilder::new(r"required\s+status\s+.*(pending|expected)")
			.case_insensitive(true)
			.build()
			.unwrap();

	if missing_status_matcher.find(msg).is_some() {
		// This problem will be solved automatically when all the required statuses are delivered, thus
		// it can be ignored here
		log::info!(
			"Ignoring merge failure due to pending required status; message: {}",
			msg
		);
		return Ok(Err(Error::MergeFailureWillBeSolvedLater {
			msg: msg.to_string(),
		}));
	}

	Err(Error::Message { msg: msg.into() })
}

fn format_error(_state: &AppState, err: Error) -> String {
	match err {
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
		err => {
			if let Error::WithIssue {
				source,
				issue: (owner, repo, number),
				..
			} = err
			{
				match *source {
					Error::MergeFailureWillBeSolvedLater { .. } => (),
					err => {
						let msg = {
							let description = format_error(state, err);
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
				}
			}
		}
	}
}
