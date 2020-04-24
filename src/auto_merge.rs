use crate::{bots, constants::*, github, process, Result};
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
	/// Cancelled or not requested
	Declined,
}

impl bots::Bot {
	/// Returns `true` if the pull request has been approved by the project owner or a minimum
	/// number of core developers.
	fn pull_request_is_approved(
		&self,
		process_info: &process::CombinedProcessInfo,
		reviews: &[github::Review],
	) -> bool {
		let owner_approved = reviews
			.iter()
			.sorted_by_key(|r| r.submitted_at)
			.rev()
			.find(|r| process_info.is_linked_owner(&r.user.login))
			.map_or(false, |r| r.state == Some(github::ReviewState::Approved));

		let core_approved = reviews
			.iter()
			.filter(|r| {
				self.core_devs.iter().any(|u| &u.login == &r.user.login)
					&& r.state == Some(github::ReviewState::Approved)
			})
			.count() >= self.config.min_reviewers;

		owner_approved || core_approved
	}

	/// Scans comments for auto-merge request.
	/// If the last request is more recent than the last cancel, return details of the last
	/// request.
	async fn auto_merge_requested(
		&self,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Option<AutoMergeRequest>> {
		let comments = self
			.github_bot
			.get_issue_comments(repo_name, issue_number)
			.await?;

		let last_request = comments.iter().rev().find(|c| {
			dbg!(&c);
			c.body.as_ref().map_or(false, |b| {
				b.to_lowercase().trim()
					== AUTO_MERGE_REQUEST.to_lowercase().trim()
			})
		});

		let last_cancel = comments.iter().rev().find(|c| {
			c.body.as_ref().map_or(false, |b| {
				b.to_lowercase().trim()
					== AUTO_MERGE_REQUEST_CANCELLED.to_lowercase().trim()
					|| b.to_lowercase().trim()
						== AUTO_MERGE_REQUEST_COMPLETE.to_lowercase().trim()
			})
		});

		if last_request.map(|x| x.created_at)
			> last_cancel.map(|x| x.created_at)
		{
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
		&self,
		repository: &github::Repository,
		pull_request: &github::PullRequest,
		status: &github::CombinedStatus,
	) -> Result<AutoMergeState> {
		Ok(
			if let Some(merge_request) = self
				.auto_merge_requested(&repository.name, pull_request.number)
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
						github::StatusState::Success => AutoMergeState::Ready(
							merge_request.user.login.clone(),
						),
						github::StatusState::Pending => AutoMergeState::Pending,
						github::StatusState::Failure => AutoMergeState::Failed,
					},
				)
			} else {
				AutoMergeState::Declined
			},
		)
	}

	async fn auto_merge_complete(
		&self,
		repository: &github::Repository,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		self.github_bot
			.merge_pull_request(&repository.name, pull_request)
			.await?;
		self.github_bot
			.create_issue_comment(
				&repository.name,
				pull_request.number,
				AUTO_MERGE_REQUEST_COMPLETE,
			)
			.await?;
		// TODO delete branch
		Ok(())
	}

	/// Merge the PR if it has sufficient approvals and a valid merge request is pending.
	/// Return `Ok(true)` if the merge was successful.
	pub async fn auto_merge_if_ready(
		&self,
		repository: &github::Repository,
		pull_request: &github::PullRequest,
		status: &github::CombinedStatus,
		process: &process::CombinedProcessInfo,
		reviews: &[github::Review],
	) -> Result<bool> {
		let state = self
			.auto_merge_state(&repository, &pull_request, &status)
			.await?;
		let mut merged = false;
		match state {
			AutoMergeState::Ready(requested_by) => {
				if pull_request.mergeable.unwrap_or(false)
					&& (self.pull_request_is_approved(&process, &reviews)
						|| process.is_primary_owner(&requested_by))
				{
					log::info!(
						"{} has necessary approvals; merging.",
						pull_request.html_url
					);
					self.auto_merge_complete(&repository, &pull_request)
						.await?;
					merged = true;
				} else {
					log::info!(
						"{} lacks approval; cannot merge.",
						pull_request.html_url
					);
					self.github_bot
						.create_issue_comment(
							&repository.name,
							pull_request.number,
							&AUTO_MERGE_LACKS_APPROVAL.replace(
								"{min_reviewers}",
								&format!("{}", self.config.min_reviewers),
							),
						)
						.await?;
					self.github_bot
						.create_issue_comment(
							&repository.name,
							pull_request.number,
							AUTO_MERGE_REQUEST_CANCELLED,
						)
						.await?;
				}
			}
			AutoMergeState::Pending => {
				log::info!("{} checks pending.", pull_request.html_url);
			}
			AutoMergeState::Failed => {
				log::info!(
					"{} failed checks; cannot merge.",
					pull_request.html_url
				);
				self.github_bot
					.create_issue_comment(
						&repository.name,
						pull_request.number,
						AUTO_MERGE_CHECKS_FAILED,
					)
					.await?;
				self.github_bot
					.create_issue_comment(
						&repository.name,
						pull_request.number,
						AUTO_MERGE_REQUEST_CANCELLED,
					)
					.await?;
			}
			AutoMergeState::Declined => {}
		}
		Ok(merged)
	}
}
