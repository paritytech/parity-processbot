use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn issue_events(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::IssueEvent>> {
		self.client
			.get_all(format!(
				"{base_url}/repos/{owner}/{repo_name}/issues/{issue_number}/events",
				base_url = Self::BASE_URL,
				owner = owner,
				repo_name = repo_name,
				issue_number = issue_number
			))
			.await
	}

	pub async fn create_issue_comment(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
		comment: &str,
	) -> Result<()> {
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_number}/comments",
			base = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			issue_number = issue_number
		);
		self.client
			.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}
}
