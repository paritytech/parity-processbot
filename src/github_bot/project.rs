use crate::{error, github, Result};

use snafu::{OptionExt, ResultExt};

use super::GithubBot;

use itertools::Itertools;

impl GithubBot {
	/// Returns projects associated with a repository.
	pub async fn projects(
		&self,
		owner: &str,
		repo_name: &str,
	) -> Result<Vec<github::Project>> {
		self.client
			.get_all(&format!(
				"{base_url}/repos/{owner}/{repo_name}/projects",
				base_url = github::base_api_url(),
				owner = owner,
				repo_name = repo_name,
			))
			.await
	}

	pub async fn project(
		&self,
		card: &github::ProjectCard,
	) -> Result<github::Project> {
		let url = card.project_url.as_ref().context(error::MissingData)?;
		self.client.get(url).await
	}

	pub async fn project_columns(
		&self,
		project: &github::Project,
	) -> Result<Vec<github::ProjectColumn>> {
		self.client
			.get_all(project.columns_url.as_ref().context(error::MissingData)?)
			.await
	}

	pub async fn project_column_by_name<A>(
		&self,
		project: &github::Project,
		column_name: A,
	) -> Result<Option<github::ProjectColumn>>
	where
		A: AsRef<str>,
	{
		self.project_columns(project).await.map(|columns| {
			columns.into_iter().find(|c| {
				c.name
					.as_ref()
					.map(|name| {
						name.to_lowercase()
							== column_name.as_ref().to_lowercase()
					})
					.unwrap_or(false)
			})
		})
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

	pub async fn create_project_card<A>(
		&self,
		column_id: A,
		content_id: i64,
		content_type: github::ProjectCardContentType,
	) -> Result<github::ProjectCard>
	where
		A: std::fmt::Display,
	{
		let url = format!(
			"{base}/projects/columns/{column_id}/cards",
			base = github::base_api_url(),
			column_id = column_id,
		);
		let parameters = serde_json::json!({ "content_id": content_id, "content_type": content_type });
		self.client
			.post_response(&url, &parameters)
			.await?
			.json()
			.await
			.context(error::Http)
	}

	pub async fn delete_project_card<A>(&self, card_id: A) -> Result<()>
	where
		A: std::fmt::Display,
	{
		let url = format!(
			"{base}/projects/columns/cards/{card_id}",
			base = github::base_api_url(),
			card_id = card_id,
		);
		self.client
			.delete_response(&url, &serde_json::json!({}))
			.await
			.map(|_| ())
	}
}

/*
#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_projects() {
		dotenv::dotenv().ok();

		let installation = dotenv::var("TEST_INSTALLATION_LOGIN")
			.expect("TEST_INSTALLATION_LOGIN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");
		let test_repo_name =
			dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			let created_issue = github_bot
				.create_issue(
					&test_repo_name,
					&"testing issue".to_owned(),
					&"this is a test".to_owned(),
					&"sjeohp".to_owned(),
				)
				.await
				.expect("create_issue");

			let projects = github_bot
				.projects(&test_repo_name)
				.await
				.expect("projects");
			let project = projects.first().expect("projects first");
			let backlog_column = github_bot
				.project_column_by_name(
					project,
					crate::constants::BACKLOG_DEFAULT_NAME,
				)
				.await
				.expect("project_column_by_name")
				.expect("project_column_by_name is some");

			let created_card = github_bot
				.create_project_card(
					backlog_column.id,
					created_issue.id,
					github::ProjectCardContentType::Issue,
				)
				.await
				.expect("create_project_card");
			assert_eq!(
				project.id,
				github_bot
					.project(&created_card)
					.await
					.expect("project with card")
					.id
			);

			let project_card = github_bot
				.active_project_events(&test_repo_name, created_issue.number)
				.await
				.expect("active_project_event")
				.first()
				.expect("active_project_event vec")
				.project_card
				.clone()
				.expect("project card");
			assert_eq!(
				project.id,
				github_bot
					.project(&project_card)
					.await
					.expect("project with card")
					.id
			);

			github_bot
				.delete_project_card(project_card.id.expect("project card id"))
				.await
				.expect("delete_project_card");

			github_bot
				.close_issue(&test_repo_name, created_issue.number)
				.await
				.expect("close_pull_request");
		});
	}
}
*/
