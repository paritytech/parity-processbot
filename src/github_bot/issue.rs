use crate::{error, github, Result};

use snafu::ResultExt;

use super::GithubBot;

impl GithubBot {
	/// Returns all of the issues in a single repository.
	pub async fn issues(
		&self,
		repo: &github::Repository,
	) -> Result<Vec<github::Issue>> {
		self.client
			.get_all(repo.issues_url.replace("{/number}", ""))
			.await
	}

	/// Returns the issue associated with a pull request.
	pub async fn issue(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Option<github::Issue>> {
		if let Some(github::IssueLink { href }) = pull_request
			.links
			.as_ref()
			.and_then(|links| links.issue_link.as_ref())
		{
			self.client.get(href).await.map(Some)
		} else {
			Ok(None)
		}
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

	pub async fn create_issue<A>(
		&self,
		repo_name: A,
		title: A,
		body: A,
		assignee: A,
	) -> Result<github::Issue>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name.as_ref(),
		);
		let params = serde_json::json!({
						"title": title.as_ref(),
						"body": body.as_ref(),
						"assignee": assignee.as_ref(),
		});
		self.client
			.post_response(&url, &params)
			.await?
			.json()
			.await
			.context(error::Http)
	}

	pub async fn assign_issue<A, B>(
		&self,
		repo_name: A,
		issue_id: i64,
		assignee_login: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let url = format!(
			"{base_url}/{repo}/issues/{issue_id}/assignees",
			base_url = &self.organization.repos_url,
			repo = repo_name.as_ref(),
			issue_id = issue_id
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
		repo_name: A,
		issue_number: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues/{issue_number}",
			base_url = Self::BASE_URL,
			owner = &self.organization.login,
			repo = repo_name.as_ref(),
			issue_number = issue_number
		);
		self.client
			.patch_response(&url, &serde_json::json!({ "state": "closed" }))
			.await
			.map(|_| ())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_issues() {
		dotenv::dotenv().ok();

		let github_organization =
			dotenv::var("GITHUB_ORGANIZATION").expect("GITHUB_ORGANIZATION");
		let github_token = dotenv::var("GITHUB_TOKEN").expect("GITHUB_TOKEN");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot =
				GithubBot::new(&github_organization, &github_token)
					.await
					.expect("github_bot");
			let repo = github_bot
				.repository("parity-processbot")
				.await
				.expect("repository");
			let created = github_bot
				.create_issue(
					"parity-processbot",
					"testing issue",
					"this is a test",
					"sjeohp",
				)
				.await
				.expect("create_pull_request");
			let issues = github_bot.issues(&repo).await.expect("issues");
			assert!(issues.iter().any(|is| is.title == "testing issue"));
			github_bot
				.close_issue(
					"parity-processbot",
					created.number.expect("created issue number"),
				)
				.await
				.expect("close_pull_request");
			let issues = github_bot.issues(&repo).await.expect("pull_requests");
			assert!(!issues.iter().any(|pr| pr.title == "testing issue"));
		});
	}
}
