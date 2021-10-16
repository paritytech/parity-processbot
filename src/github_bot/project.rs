use crate::{github, Result};

use super::GithubBot;

use itertools::Itertools;

impl GithubBot {
	pub async fn projects(
		&self,
		owner: &str,
		repo_name: &str,
	) -> Result<Vec<github::Project>> {
		self.client
			.get_all(&format!(
				"{base_url}/repos/{owner}/{repo_name}/projects",
				base_url = Self::BASE_URL,
				owner = owner,
				repo_name = repo_name,
			))
			.await
	}

	/// Return the most recent AddedToProject event that is not followed by a RemovedFromProject
	/// event.
	pub async fn active_project_events(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::IssueEvent>> {
		let events = self
			.issue_events(owner, repo_name, issue_number)
			.await?
			.into_iter()
			.filter(|issue_event| {
				issue_event.project_card.is_some()
					&& (issue_event.event
						== Some(github::Event::RemovedFromProject)
						|| issue_event.event
							== Some(github::Event::AddedToProject))
			})
			.collect::<Vec<github::IssueEvent>>();
		let active_project_events = events
			.iter()
			.cloned()
			.unique_by(|a| a.project_card.as_ref().map(|p| p.id))
			.filter(|a| {
				// filter for unique projects with more 'added' than 'removed' events
				events
					.iter()
					.filter(|b| {
						b.project_card == a.project_card
							&& b.event == Some(github::Event::AddedToProject)
					})
					.count() > events
					.iter()
					.filter(|b| {
						b.project_card == a.project_card
							&& b.event
								== Some(github::Event::RemovedFromProject)
					})
					.count()
			})
			.collect::<Vec<github::IssueEvent>>();
		Ok(active_project_events)
	}
}
