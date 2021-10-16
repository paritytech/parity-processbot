use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn pull_request(
		&self,
		owner: &str,
		repo_name: &str,
		pull_number: i64,
	) -> Result<github::PullRequest> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
				base_url = Self::BASE_URL,
				owner = owner,
				repo = repo_name,
				pull_number = pull_number
			))
			.await
	}

	pub async fn pull_request_with_head(
		&self,
		owner: &str,
		repo_name: &str,
		head: &str,
	) -> Result<Option<github::PullRequest>> {
		self.client
			.get_all(format!(
				"{base_url}/repos/{owner}/{repo}/pulls?head={head}",
				base_url = Self::BASE_URL,
				owner = owner,
				repo = repo_name,
				head = head,
			))
			.await
			.map(|v| v.first().cloned())
	}

	pub async fn merge_pull_request(
		&self,
		owner: &str,
		repo_name: &str,
		number: i64,
		head_sha: &str,
	) -> Result<()> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{number}/merge",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			number = number,
		);
		let params = serde_json::json!({
			"sha": head_sha,
			"merge_method": "squash"
		});
		self.client.put_response(&url, &params).await.map(|_| ())
	}
}
