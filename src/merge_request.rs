use std::collections::HashMap;

use hyper::StatusCode as HttpStatusCode;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::{
	companion::{
		check_all_companions_are_mergeable, CompanionReferenceTrailItem,
	},
	core::{
		get_commit_checks, get_commit_statuses, process_dependents_after_merge,
		AppState, Status,
	},
	error::{self, Error},
	github::GithubPullRequest,
	types::Result,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(C)]
pub struct MergeRequestDependency {
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
	pub dependencies: Option<Vec<MergeRequestDependency>>,
}

pub enum MergeRequestCleanupReason<'a> {
	AfterMerge,
	AfterSHAUpdate(&'a String),
	Cancelled,
	Error,
}
// Removes a pull request from the database (e.g. when it has been merged) and
// executes side-effects related to the kind of trigger for this function
pub async fn cleanup_merge_request(
	state: &AppState,
	key_to_guarantee_deleted: &str,
	owner: &str,
	repo: &str,
	number: i64,
	reason: &MergeRequestCleanupReason<'_>,
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
							"Failed to delete {} during cleanup_merge_request due to {:?}",
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
		log::info!(
			"Acquiring cleanup_merge_request's recursion prevention lock"
		);
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
		log::info!(
			"Releasing cleanup_merge_request's recursion prevention lock"
		);
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
		MergeRequestCleanupReason::Error
		| MergeRequestCleanupReason::Cancelled => {
			for dependent in related_dependents.values() {
				let _ = cleanup_merge_request(
					state,
					&dependent.sha,
					&dependent.owner,
					&dependent.repo,
					dependent.number,
					reason,
				);
			}
		}
		MergeRequestCleanupReason::AfterSHAUpdate(updated_sha) => {
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
		MergeRequestCleanupReason::AfterMerge => {}
	}

	log::info!(
		"Cleaning up cleanup_merge_request recursion prevention lock's entries"
	);
	CLEANUP_PR_RECURSION_PREVENTION.lock().clear();

	Ok(())
}

pub enum MergeRequestQueuedMessage<'a> {
	Custom(&'a str),
	Default,
	None,
}
pub async fn queue_merge_request(
	state: &AppState,
	mr: &MergeRequest,
	msg: &MergeRequestQueuedMessage<'_>,
) -> Result<()> {
	register_merge_request(state, mr).await?;

	let AppState { gh_client, .. } = state;

	let MergeRequest {
		owner,
		repo,
		number,
		..
	} = mr;

	let msg = match msg {
		MergeRequestQueuedMessage::Custom(msg) => msg,
		MergeRequestQueuedMessage::Default => "Waiting for commit status.",
		MergeRequestQueuedMessage::None => return Ok(()),
	};

	let post_comment_result = gh_client
		.create_issue_comment(owner, repo, *number, msg)
		.await;
	if let Err(err) = post_comment_result {
		log::error!("Error posting comment: {}", err);
	}

	Ok(())
}

pub async fn handle_merged_pull_request(
	state: &AppState,
	pr: &GithubPullRequest,
	requested_by: &str,
) -> Result<bool> {
	if !pr.merged {
		return Ok(false);
	}

	let was_cleaned_up = cleanup_merge_request(
		state,
		&pr.head.sha,
		&pr.base.repo.owner.login,
		&pr.base.repo.name,
		pr.number,
		&MergeRequestCleanupReason::AfterMerge,
	)
	.await
	.map(|_| true);

	/*
		It's not sane to try to handle the dependents if the cleanup went wrong since
		that hints at some bug in the application
	*/
	if was_cleaned_up.is_ok() {
		if let Err(err) =
			process_dependents_after_merge(state, pr, requested_by).await
		{
			log::error!(
				"Failed to process process_dependents_after_merge in cleanup_merged_pr due to {:?}",
				err
			);
		}
	}

	was_cleaned_up
}

pub async fn is_ready_to_merge(
	state: &AppState,
	pr: &GithubPullRequest,
) -> Result<bool> {
	let AppState { gh_client, .. } = state;

	match get_commit_checks(
		gh_client,
		&pr.base.repo.owner.login,
		&pr.base.repo.name,
		&pr.head.sha,
		&pr.html_url,
	)
	.await?
	{
		Status::Success => {
			match get_commit_statuses(
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
				Status::Failure => Err(Error::StatusesFailed {
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

pub async fn merge_pull_request(
	state: &AppState,
	pr: &GithubPullRequest,
	requested_by: &str,
) -> Result<Result<()>> {
	if handle_merged_pull_request(state, pr, requested_by).await? {
		return Ok(Ok(()));
	}

	let AppState { gh_client, .. } = state;

	let err = match gh_client
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
			if let Err(err) = cleanup_merge_request(
				state,
				&pr.head.sha,
				&pr.base.repo.owner.login,
				&pr.base.repo.name,
				pr.number,
				&MergeRequestCleanupReason::AfterMerge,
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
		} if *status == HttpStatusCode::METHOD_NOT_ALLOWED => {
			match body.get("message") {
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
			}
		}
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

pub async fn check_merge_is_allowed(
	state: &AppState,
	pr: &GithubPullRequest,
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
