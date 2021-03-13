use crate::{error, github, Result};

use snafu::ResultExt;

use super::GithubBot;

impl GithubBot {
	/// Returns a single issue.
	pub async fn issue(
		&self,
		owner: &str,
		repo: &github::Repository,
		number: i64,
	) -> Result<github::Issue> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/issues/{number}",
				base_url = github::base_api_url(),
				owner = owner,
				repo = repo.name,
				number = number
			))
			.await
	}

	/// Returns events associated with an issue.
	pub async fn issue_events(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::IssueEvent>> {
		self.client
			.get_all(format!(
				"{base_url}/repos/{owner}/{repo_name}/issues/{issue_number}/events",
				base_url = github::base_api_url(),
				owner = owner,
				repo_name = repo_name,
				issue_number = issue_number
			))
			.await
	}

	pub async fn issue_projects<'a>(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
		projects: &'a [github::Project],
	) -> Result<Vec<&'a github::Project>> {
		self.active_project_events(owner, repo_name, issue_number)
			.await
			.map(|v| {
				v.iter()
					.filter_map(|issue_event| {
						projects.iter().find(|proj| {
							issue_event
								.project_card
								.as_ref()
								.expect("issue event project card")
								.id == proj.id
						})
					})
					.collect::<Vec<&'a github::Project>>()
			})
	}

	pub async fn create_issue(
		&self,
		owner: &str,
		repo_name: &str,
		title: &str,
		body: &str,
		assignee: &str,
	) -> Result<github::Issue> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name,
		);
		let params = serde_json::json!({
						"title": title,
						"body": body,
						"assignee": assignee,
		});
		self.client
			.post_response(&url, &params)
			.await?
			.json()
			.await
			.context(error::Http)
	}

	/// Adds a comment to an issue.
	pub async fn create_issue_comment(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
		comment: &str,
	) -> Result<()> {
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_number}/comments",
			base = github::base_api_url(),
			owner = owner,
			repo = repo_name,
			issue_number = issue_number
		);
		self.client
			.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}

	pub async fn assign_issue<A, B>(
		&self,
		owner: &str,
		repo_name: A,
		issue_number: i64,
		assignee_login: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues/{issue_number}/assignees",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name.as_ref(),
			issue_number = issue_number
		);
		self.client
			.post_response(
				&url,
				&serde_json::json!({ "assignees": [assignee_login.as_ref()] }),
			)
			.await
			.map(|_| ())
	}

	pub async fn close_issue<A>(
		&self,
		owner: &str,
		repo_name: A,
		issue_number: i64,
	) -> Result<github::Issue>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues/{issue_number}",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name.as_ref(),
			issue_number = issue_number
		);
		self.client
			.patch_response(&url, &serde_json::json!({ "state": "closed" }))
			.await?
			.json()
			.await
			.context(error::Http)
	}
}
