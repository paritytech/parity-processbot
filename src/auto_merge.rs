use crate::{
	config::BotConfig, constants::*, github, github_bot::GithubBot, process,
	Result,
};
use itertools::Itertools;

#[derive(Debug, Clone)]
pub struct AutoMergeRequest {
	user: github::User,
	requested_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoMergeState {
	/// PR can be auto-merged
	Ready(String),
	/// Checks are pending
	Pending,
	/// Checks failed or PR was not mergeable
	Failed,
	/// Status error
	Error,
	/// Cancelled or not requested
	Declined,
}

/// Returns `true` if the pull request has been approved by the project owner or a minimum
/// number of core developers.
fn pull_request_is_approved(
	config: &BotConfig,
	core_devs: &[String],
	process_info: &process::CombinedProcessInfo,
	reviews: &[github::Review],
) -> bool {
	let owner_approved = reviews
		.iter()
		.sorted_by_key(|r| r.submitted_at)
		.rev()
		.find(|r| process_info.is_owner(&r.user.login))
		.map_or(false, |r| r.state == Some(github::ReviewState::Approved));

	let core_approved = reviews
		.iter()
		.filter(|r| {
			core_devs.iter().any(|u| u == &r.user.login)
				&& r.state == Some(github::ReviewState::Approved)
		})
		.count() >= config.min_reviewers;

	owner_approved || core_approved
}

/// Scans comments for auto-merge request.
/// If the last request is more recent than the last cancel, return details of the last
/// request.
async fn auto_merge_requested(
	github_bot: &GithubBot,
	repo_name: &str,
	issue_number: i64,
) -> Result<Option<AutoMergeRequest>> {
	let comments = github_bot
		.get_issue_comments(repo_name, issue_number)
		.await?;

	let last_request = comments.iter().rev().find(|c| {
		c.body.to_lowercase().trim() == AUTO_MERGE_REQUEST.to_lowercase().trim()
	});

	let last_cancel = comments.iter().rev().find(|c| {
		c.body.to_lowercase().trim()
			== AUTO_MERGE_REQUEST_CANCELLED.to_lowercase().trim()
			|| c.body.to_lowercase().trim()
				== AUTO_MERGE_REQUEST_COMPLETE.to_lowercase().trim()
	});

	if last_request.map(|x| x.created_at) > last_cancel.map(|x| x.created_at) {
		Ok(last_request.map(|x| AutoMergeRequest {
			user: x.user.clone(),
			requested_at: x.created_at,
		}))
	} else {
		Ok(None)
	}
}

/// Returns the state of any pending auto-merge for a given PR.
async fn auto_merge_state(
	github_bot: &GithubBot,
	repo_name: &str,
	pull_request: &github::PullRequest,
	status: &github::CombinedStatus,
) -> Result<AutoMergeState> {
	Ok(
		if let Some(merge_request) =
			auto_merge_requested(github_bot, &repo_name, pull_request.number)
				.await?
		{
			let last_status = status
				.statuses
				.iter()
				.sorted_by_key(|x| x.created_at)
				.rev()
				.take(1)
				.last();

			last_status.map_or(
				AutoMergeState::Ready(merge_request.user.login.clone()),
				|s| match s.state {
					github::StatusState::Success => {
						AutoMergeState::Ready(merge_request.user.login.clone())
					}
					github::StatusState::Pending => AutoMergeState::Pending,
					github::StatusState::Failure => AutoMergeState::Failed,
					github::StatusState::Error => AutoMergeState::Error,
				},
			)
		} else {
			AutoMergeState::Declined
		},
	)
}

async fn auto_merge_complete(
	github_bot: &GithubBot,
	repo_name: &str,
	pull_request: &github::PullRequest,
) -> Result<()> {
	github_bot
		.merge_pull_request(
			&repo_name,
			pull_request.number,
			&pull_request.head.sha,
		)
		.await?;
	Ok(())
}

pub async fn auto_merge_if_approved(
	github_bot: &GithubBot,
	config: &BotConfig,
	core_devs: &[String],
	repo_name: &str,
	pull_request: &github::PullRequest,
	process: &process::CombinedProcessInfo,
	reviews: &[github::Review],
	requested_by: &str,
) -> Result<bool> {
	let mergeable = pull_request.mergeable.unwrap_or(false);
	let approved =
		pull_request_is_approved(config, core_devs, &process, &reviews);
	let owner_request = process.is_owner(&requested_by);
	if mergeable && (approved || owner_request) {
		log::info!(
			"{} has necessary approvals; merging.",
			pull_request.html_url
		);
		auto_merge_complete(&github_bot, &repo_name, &pull_request).await?;
		Ok(true)
	} else {
		if !mergeable {
			log::info!("{} is unmergeable.", pull_request.html_url);
		}
		if !(approved || owner_request) {
			log::info!(
				"{} lacks approval; cannot merge.",
				pull_request.html_url
			);
		}
		github_bot
			.create_issue_comment(
				&repo_name,
				pull_request.number,
				&AUTO_MERGE_FAILED.replace(
					"{min_reviewers}",
					&format!("{}", config.min_reviewers),
				),
			)
			.await?;
		github_bot
			.create_issue_comment(
				&repo_name,
				pull_request.number,
				AUTO_MERGE_REQUEST_CANCELLED,
			)
			.await?;
		Ok(false)
	}
}

/// Merge the PR if it has sufficient approvals and a valid merge request is pending.
/// Return `Ok(true)` if the merge was successful.
pub async fn auto_merge_if_ready(
	github_bot: &GithubBot,
	config: &BotConfig,
	core_devs: &[String],
	repo_name: &str,
	pull_request: &github::PullRequest,
	status: &github::CombinedStatus,
	process: &process::CombinedProcessInfo,
	reviews: &[github::Review],
) -> Result<bool> {
	let state =
		auto_merge_state(github_bot, &repo_name, &pull_request, &status)
			.await?;
	let mut merged = false;
	match state {
		AutoMergeState::Ready(requested_by) => {
			merged = auto_merge_if_approved(
				github_bot,
				config,
				core_devs,
				repo_name,
				pull_request,
				process,
				reviews,
				&requested_by,
			)
			.await?;
		}
		AutoMergeState::Pending => {
			log::info!("{} checks pending.", pull_request.html_url);
		}
		AutoMergeState::Failed => {
			log::info!(
				"{} failed checks; cannot merge.",
				pull_request.html_url
			);
			github_bot
				.create_issue_comment(
					&repo_name,
					pull_request.number,
					AUTO_MERGE_CHECKS_FAILED,
				)
				.await?;
			github_bot
				.create_issue_comment(
					&repo_name,
					pull_request.number,
					AUTO_MERGE_REQUEST_CANCELLED,
				)
				.await?;
		}
		AutoMergeState::Error => {
			log::info!("{} status error; aborting.", pull_request.html_url);
			github_bot
				.create_issue_comment(
					&repo_name,
					pull_request.number,
					AUTO_MERGE_CHECKS_ERROR,
				)
				.await?;
			github_bot
				.create_issue_comment(
					&repo_name,
					pull_request.number,
					AUTO_MERGE_REQUEST_CANCELLED,
				)
				.await?;
		}
		AutoMergeState::Declined => {}
	}
	Ok(merged)
}
