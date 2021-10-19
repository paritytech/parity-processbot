use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn pull_request(
		&self,
		owner: &str,
		repo: &str,
		number: i64,
	) -> Result<github::PullRequest> {
		self.client
			.get(format!(
				"{}/repos/{}/{}/pulls/{}",
				self.github_api_url, owner, repo, number
			))
			.await
	}

	pub async fn pull_request_with_head(
		&self,
		owner: &str,
		repo: &str,
		head: &str,
	) -> Result<Option<github::PullRequest>> {
		self.client
			.get_all(format!(
				"{}/repos/{}/{}/pulls?head={}",
				self.github_api_url, owner, repo, head
			))
			.await
			.map(|v| v.first().cloned())
	}

	pub async fn merge_pull_request(
		&self,
		owner: &str,
		repo: &str,
		number: i64,
		head_sha: &str,
	) -> Result<()> {
		let url = format!(
			"{}/repos/{}/{}/pulls/{}/merge",
			self.github_api_url, owner, repo, number
		);
		let params = serde_json::json!({
			"sha": head_sha,
			"merge_method": "squash"
		});
		self.client.put_response(&url, &params).await.map(|_| ())
	}
}
