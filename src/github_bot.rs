use crate::{error, github, Result};

use snafu::OptionExt;

use serde::Serialize;

pub struct GithubBot {
	client: crate::http::Client,
	organization: github::Organization,
}

impl GithubBot {
	const BASE_URL: &'static str = "https://api.github.com";

	/// Creates a new instance of `GithubBot` from a GitHub organization defined
	/// by `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organization does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub async fn new<A: AsRef<str>, I: Into<String>>(
		org: A,
		auth_key: I,
	) -> Result<Self> {
		let client = crate::http::Client::new(auth_key);

		let organization = client
			.get(&format!("{}/orgs/{}", Self::BASE_URL, org.as_ref()))
			.await?;

		Ok(Self {
			client,
			organization,
		})
	}

	/// Returns all of the repositories managed by the organization.
	pub async fn repositories(&self) -> Result<Vec<github::Repository>> {
		self.client.get_all(&self.organization.repos_url).await
	}

	/// Returns all of the pull requests in a single repository.
	pub async fn pull_requests(
		&self,
		repo: &github::Repository,
	) -> Result<Vec<github::PullRequest>> {
		self.client
			.get_all(repo.pulls_url.replace("{/number}", ""))
			.await
	}

	/// Returns all of the issues in a single repository.
	pub async fn issues(
		&self,
		repo: &github::Repository,
	) -> Result<Vec<github::Issue>> {
		self.client
			.get_all(repo.issues_url.replace("{/number}", ""))
			.await
	}

	/// Returns all reviews associated with a pull request.
	pub async fn reviews(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Vec<github::Review>> {
		let url = pull_request.html_url.as_ref().context(error::MissingData)?;

		self.client.get_all(format!("{}/reviews", url)).await
	}

	/// Requests a review from a user.
	pub async fn request_reviews(
		&self,
		repo_name: &str,
		pull_number: i64,
		reviewers: &[&str],
	) -> Result<github::PullRequest> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo_name}/pulls/{pull_number}/requested_reviewers",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo_name = repo_name,
			pull_number = pull_number
		);
		let body = &serde_json::json!({ "reviewers": reviewers });

		self.client.post(url, body).await
	}

	/// Returns the issue associated with a pull request.
	pub async fn issue(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Option<github::Issue>> {
		if let Some(github::IssueLink { href }) = &pull_request.links.issue_link
		{
			self.client.get(href).await.map(Some)
		} else {
			Ok(None)
		}
	}

	pub async fn project(
		&self,
		card: &github::ProjectCard,
	) -> Result<github::Project> {
		let url = card.project_url.as_ref().context(error::MissingData)?;
		self.client.get(url).await
	}

	pub async fn project_column(
		&self,
		card: &github::ProjectCard,
	) -> Result<github::ProjectColumn> {
		self.client
			.get(card.column_url.as_ref().context(error::MissingData)?)
			.await
	}

	pub async fn project_columns(
		&self,
		project: &github::Project,
	) -> Result<Vec<github::ProjectColumn>> {
		self.client
			.get(project.columns_url.as_ref().context(error::MissingData)?)
			.await
	}

	/// Returns events associated with an issue.
	pub async fn issue_events(
		&self,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::IssueEvent>> {
		self.client
			.get(format!(
			"{base_url}/repos/{owner}/{repo_name}/issues/{issue_number}/events",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo_name = repo_name,
			issue_number = issue_number
		))
			.await
	}

	/// Returns events associated with an issue.
	pub async fn projects(
		&self,
		repo_name: &str,
	) -> Result<Vec<github::Project>> {
		self.client
			.get(&format!(
				"{base_url}/repos/{owner}/{repo_name}/projects",
				base_url = Self::BASE_URL,
				owner = self.organization.login,
				repo_name = repo_name,
			))
			.await
	}

	/// Returns statuses associated with a pull request.
	pub async fn statuses(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Option<Vec<github::Status>>> {
		if let Some(github::StatusesLink { href }) =
			&pull_request.links.statuses_link
		{
			self.client.get(href).await.map(Some)
		} else {
			Ok(None)
		}
	}

	/// Returns the contents of a file in a repository.
	pub async fn contents(
		&self,
		repo_name: &str,
		path: &str,
	) -> Result<github::Contents> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo_name}/contents/{path}",
				base_url = Self::BASE_URL,
				owner = self.organization.login,
				repo_name = repo_name,
				path = path
			))
			.await
	}

	/// Returns the team with a given team slug (eg. 'core-devs').
	pub async fn team(&self, slug: &str) -> Result<github::Team> {
		let url = self.organization.url.as_ref().context(error::MissingData)?;

		self.client.get(format!("{}/teams/{}", url, slug)).await
	}

	/// Returns members of the team with a id.
	pub async fn team_members(
		&self,
		team_id: i64,
	) -> Result<Vec<github::User>> {
		self.client
			.get(format!("{}/teams/{}/members", Self::BASE_URL, team_id))
			.await
	}

	/// Creates a comment in the repo
	pub async fn add_comment<A, B>(
		&self,
		repo_name: A,
		issue_id: i64,
		comment: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		log::info!("Adding comment");
		let repo = repo_name.as_ref();
		let comment = comment.as_ref();
		let url = format!(
			"{base}/repos/{org}/{repo}/issues/{issue_id}/comments",
			base = Self::BASE_URL,
			org = self.organization.login,
			repo = repo,
			issue_id = issue_id
		);
		log::info!("POST {}", url);

		self.client
			.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}

	pub async fn assign_author<A, B>(
		&self,
		repo_name: A,
		issue_id: i64,
		author_login: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let author = author_login.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/{repo}/issues/{issue_id}/assignees",
			base = base,
			repo = repo,
			issue_id = issue_id
		);

		self.client
			.post_response(&url, &serde_json::json!({ "assignees": [author] }))
			.await
			.map(|_| ())
	}

	pub async fn merge_pull_request<A>(
		&self,
		repo_name: A,
		pull_number: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/pulls/{pull_number}/merge",
			base = base,
			owner = self.organization.login,
			repo = repo,
			pull_number = pull_number
		);
		self.client
			.put_response(&url, &serde_json::json!({}))
			.await
			.map(|_| ())
	}

	pub async fn close_pull_request<A>(
		&self,
		repo_name: A,
		pull_number: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/pulls/{pull_number}",
			base = base,
			owner = self.organization.login,
			repo = repo,
			pull_number = pull_number
		);
		self.client
			.patch_response(&url, &serde_json::json!({ "state": "closed" }))
			.await
			.map(|_| ())
	}

	pub async fn close_issue<A>(
		&self,
		repo_name: A,
		issue_id: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_id}",
			base = base,
			owner = self.organization.login,
			repo = repo,
			issue_id = issue_id
		);
		self.client
			.patch_response(&url, &serde_json::json!({ "state": "closed" }))
			.await
			.map(|_| ())
	}

	pub async fn create_issue<A, B>(
		&self,
		repo_name: A,
		parameters: &B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: Serialize,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues",
			base = base,
			owner = self.organization.login,
			repo = repo,
		);
		self.client
			.post_response(&url, parameters)
			.await
			.map(|_| ())
	}

	pub async fn create_project_card<A>(
		&self,
		column_id: A,
		content_id: i64,
		content_type: github::ProjectCardContentType,
	) -> Result<()>
	where
		A: std::fmt::Display,
	{
		let url = format!(
			"{base}/projects/columns/{column_id}/cards",
			base = Self::BASE_URL,
			column_id = column_id,
		);
		let parameters = serde_json::json!({ "content_id": content_id, "content_type": content_type });
		self.client
			.post_response(&url, &parameters)
			.await
			.map(|_| ())
	}

	pub async fn delete_project_card<A>(&self, column_id: A) -> Result<()>
	where
		A: std::fmt::Display,
	{
		let url = format!(
			"{base}/projects/columns/{column_id}",
			base = Self::BASE_URL,
			column_id = column_id,
		);
		self.client
			.delete_response(&url, &serde_json::json!({}))
			.await
			.map(|_| ())
	}
}
