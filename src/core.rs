use std::collections::{HashMap, HashSet};

use async_recursion::async_recursion;
use regex::RegexBuilder;
use reqwest::Client as HttpClient;
use rocksdb::DB;
use snafu::ResultExt;

use crate::{
	companion::update_companion_then_merge,
	config::MainConfig,
	error::{self, handle_error, Error, PullRequestDetails},
	git_ops::rebase,
	github::*,
	gitlab::*,
	merge_request::{
		check_merge_is_allowed, cleanup_merge_request,
		handle_merged_pull_request, is_ready_to_merge, merge_pull_request,
		queue_merge_request, MergeRequest, MergeRequestCleanupReason,
		MergeRequestQueuedMessage,
	},
	types::Result,
	vanity_service,
};

#[derive(Debug)]
pub enum Status {
	Success,
	Pending,
	Failure,
}

pub enum PullRequestMergeCancelOutcome {
	ShaNotFound,
	WasCancelled,
	WasNotCancelled,
}

pub struct AppState {
	pub db: DB,
	pub gh_client: GithubClient,
	pub config: MainConfig,
}

#[derive(Debug)]
pub enum CommentCommand {
	Merge(MergeCommentCommand),
	CancelMerge,
	Rebase,
}

#[derive(Debug)]
pub enum MergeCommentCommand {
	Normal,
	Force,
}

pub async fn get_commit_statuses(
	state: &AppState,
	owner: &str,
	repo: &str,
	commit_sha: &str,
	html_url: &str,
	should_handle_retried_jobs: bool,
) -> Result<(
	Status,
	HashMap<String, (i64, GithubCommitStatusState, Option<String>)>,
)> {
	let AppState {
		gh_client, config, ..
	} = state;

	let statuses = gh_client.statuses(owner, repo, commit_sha).await?;
	log::info!("{} statuses: {:?}", html_url, statuses);

	// Since Github only considers the latest instance of each status, we should
	// abide by the same rule. Each instance is uniquely identified by "context".
	let mut latest_statuses: HashMap<
		String,
		(i64, GithubCommitStatusState, Option<String>),
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
		.all(|(_, state, _)| *state == GithubCommitStatusState::Success)
	{
		log::info!("{} has success status", html_url);
		Ok((Status::Success, latest_statuses))
	} else if latest_statuses.values().any(|(_, state, _)| {
		*state == GithubCommitStatusState::Error
			|| *state == GithubCommitStatusState::Failure
	}) {
		if should_handle_retried_jobs {
			let mut has_failed_status_from_outside_gitlab = false;

			let gitlab_job_target_url_matcher =
				RegexBuilder::new(r"^(\w+://[^/]+)/(.*)/builds/([0-9]+)$")
					.case_insensitive(true)
					.build()
					.unwrap();
			let failed_gitlab_jobs = latest_statuses
				.values()
				.filter_map(|(_, status, target_url)| match *status {
					GithubCommitStatusState::Failure
					| GithubCommitStatusState::Error => {
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
							has_failed_status_from_outside_gitlab = true;
						}
						gitlab_job_data
					}
					_ => None,
				})
				.collect::<Vec<_>>();

			if has_failed_status_from_outside_gitlab {
				log::info!(
					"Non-GitLab statuses have failed, therefore we bail out of trying to check if following GitLab jobs have recovered: {:?}",
					failed_gitlab_jobs
				);
			} else if !failed_gitlab_jobs.is_empty() {
				let mut recovered_jobs = vec![];

				let http_client = HttpClient::new();
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
								.headers(
									config.get_gitlab_api_request_headers()?,
								)
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
						.json::<GitlabJob>()
						.await
						.context(error::Http)?;

					log::info!("Fetched job for {}: {:?}", job_api_url, job);

					match job.pipeline.status {
						GitlabPipelineStatus::Created
						| GitlabPipelineStatus::WaitingForResource
						| GitlabPipelineStatus::Preparing
						| GitlabPipelineStatus::Pending
						| GitlabPipelineStatus::Running
						| GitlabPipelineStatus::Scheduled => {
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
												.headers(config.get_gitlab_api_request_headers()?)
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
										.json::<Vec<GitlabPipelineJob>>()
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
									"{} 's GitLab pipeline (id: {}) for job {} (name: {}) did not list it as pending or successful, therefore the job is considered to be failing",
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
								"{} 's GitLab pipeline (id: {}) for job {} (name: {}) is not pending, therefore the job itself can't be considered to be pending",
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

pub async fn get_commit_checks(
	gh_client: &GithubClient,
	owner: &str,
	repo_name: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let check_runs = gh_client.check_runs(owner, repo_name, commit_sha).await?;
	log::info!("{} check_runs: {:?}", html_url, check_runs);

	// Since Github only considers the latest instance of each check, we should abide by the same
	// rule. Each instance is uniquely identified by "name".
	let mut latest_checks = HashMap::new();
	for c in check_runs {
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
			*conclusion == Some(GithubCheckRunConclusion::Success)
		}) {
			log::info!("{} has successful checks", html_url);
			Status::Success
		} else if latest_checks
			.values()
			.all(|(_, status, _)| *status == GithubCheckRunStatus::Completed)
		{
			log::info!("{} has unsuccessful checks", html_url);
			Status::Failure
		} else {
			log::info!("{} has pending checks", html_url);
			Status::Pending
		},
	)
}

#[async_recursion]
pub async fn process_commit_checks_and_statuses(
	state: &AppState,
	sha: &str,
) -> Result<()> {
	let AppState { db, gh_client, .. } = state;

	log::info!("Checking for statuses of {}", sha);

	let mr: MergeRequest = match db.get(sha.as_bytes()).context(error::Db)? {
		Some(bytes) => bincode::deserialize(&bytes).context(error::Bincode)?,
		None => return Ok(()),
	};
	let pr = gh_client
		.pull_request(&mr.owner, &mr.repo, mr.number)
		.await?;
	log::info!(
		"Deserialized merge request for {} (sha {}): {:?}",
		pr.html_url,
		sha,
		mr
	);

	match async {
		if handle_merged_pull_request(state, &pr, &mr.requested_by).await? {
			return Ok(());
		}

		if mr.sha != pr.head.sha {
			return Err(Error::HeadChanged {
				expected: sha.to_string(),
				actual: pr.head.sha.to_owned(),
			});
		}

		if !is_ready_to_merge(state, &pr).await? {
			log::info!("{} is not ready", pr.html_url);
			return Ok(());
		}

		check_merge_is_allowed(state, &pr, &mr.requested_by, &[]).await?;

		if let Some(dependencies) = &mr.dependencies {
			for dependency in dependencies {
				let dependency_pr = gh_client
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
					cleanup_merge_request(
						state,
						&dependency_pr.head.sha,
						&dependency.owner,
						&dependency.repo,
						dependency.number,
						&MergeRequestCleanupReason::AfterMerge,
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
		update_companion_then_merge(
			state,
			&mr,
			&MergeRequestQueuedMessage::None,
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
		Err(err) => Err(err.with_pull_request_details(PullRequestDetails {
			owner: pr.base.repo.owner.login,
			repo: pr.base.repo.name,
			number: pr.number,
		})),
	}
}

pub async fn process_dependents_after_merge(
	state: &AppState,
	pr: &GithubPullRequest,
	requested_by: &str,
) -> Result<()> {
	log::info!("Handling dependents of {}", pr.html_url);

	let AppState {
		gh_client,
		db,
		config,
		..
	} = state;

	let fetched_dependents = gh_client
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
									let _ = cleanup_merge_request(
										state,
										&mr.sha,
										&mr.owner,
										&mr.repo,
										mr.number,
										&MergeRequestCleanupReason::Error,
									)
									.await;
									handle_error(
										PullRequestMergeCancelOutcome::WasCancelled,
										Error::Message {
											msg: format!(
												"Unable to update {} in the database (detected as a dependent of {})",
												&mr.html_url,
												pr.html_url
											),
										}
										.with_pull_request_details(PullRequestDetails {
											owner: (&mr.owner).into(),
											repo: (&mr.repo).into(),
											number: mr.number,
										}),
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
		match update_companion_then_merge(
			state,
			dependent,
			&MergeRequestQueuedMessage::Default,
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
				let _ = cleanup_merge_request(
					state,
					&dependent.sha,
					&dependent.owner,
					&dependent.repo,
					dependent.number,
					&MergeRequestCleanupReason::Error,
				)
				.await;
				handle_error(
					PullRequestMergeCancelOutcome::WasCancelled,
					err.with_pull_request_details(PullRequestDetails {
						owner: (&dependent.owner).into(),
						repo: (&dependent.repo).into(),
						number: dependent.number,
					}),
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
						let _ = cleanup_merge_request(
							state,
							&dependent_of_dependent.sha,
							&dependent_of_dependent.owner,
							&dependent_of_dependent.repo,
							dependent_of_dependent.number,
							&MergeRequestCleanupReason::Error,
						)
						.await;
						handle_error(
							PullRequestMergeCancelOutcome::WasCancelled,
							Error::Message {
								msg: format!(
									 "Failed to update database references of {:?} in dependent {} after the merge of {}",
									 updated_dependencies,
									 dependent_of_dependent.html_url,
									 pr.html_url
								),
							}
							.with_pull_request_details(PullRequestDetails {
								owner: (&dependent_of_dependent.owner).into(),
								repo: (&dependent_of_dependent.repo).into(),
								number: dependent_of_dependent.number,
							}),
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
		if let Err(err) =
			process_commit_checks_and_statuses(state, &dependent.sha).await
		{
			let _ = cleanup_merge_request(
				state,
				&dependent.sha,
				&dependent.owner,
				&dependent.repo,
				dependent.number,
				&MergeRequestCleanupReason::Error,
			)
			.await;
			handle_error(
				PullRequestMergeCancelOutcome::WasCancelled,
				err,
				state,
			)
			.await;
		}
	}

	Ok(())
}

pub async fn handle_command(
	state: &AppState,
	cmd: &CommentCommand,
	pr: &GithubPullRequest,
	requested_by: &str,
) -> Result<()> {
	let AppState { gh_client, .. } = state;

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
					if is_ready_to_merge(state, pr).await? {
						match merge_pull_request(state, pr, requested_by)
							.await?
						{
							// If the merge failure will be solved later, then register the PR in the database so that
							// it'll eventually resume processing when later statuses arrive
							Err(Error::MergeFailureWillBeSolvedLater {
								msg,
							}) => {
								let msg = format!(
									"This PR cannot be merged **at the moment** due to: {}\n\nprocessbot expects that the problem will be solved automatically later and so the auto-merge process will be started. You can simply wait for now.\n\n",
									msg
								);
								queue_merge_request(
									state,
									&mr,
									&MergeRequestQueuedMessage::Custom(&msg),
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
						queue_merge_request(
							state,
							&mr,
							&MergeRequestQueuedMessage::Default,
						)
						.await?;
						return Ok(());
					}
				}
				MergeCommentCommand::Force => {
					match merge_pull_request(state, pr, requested_by).await? {
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

			process_dependents_after_merge(state, pr, requested_by).await
		}
		CommentCommand::CancelMerge => {
			log::info!("Deleting merge request for {}", pr.html_url);

			cleanup_merge_request(
				state,
				&pr.head.sha,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
				&MergeRequestCleanupReason::Cancelled,
			)
			.await?;

			if let Err(err) = gh_client
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
			if let Err(err) = gh_client
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
				gh_client,
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
