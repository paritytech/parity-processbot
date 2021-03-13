use crate::{error, github, Result};

use snafu::ResultExt;

use super::GithubBot;

impl GithubBot {
	/// Returns a single pull request.
	pub async fn pull_request(
		&self,
		owner: &str,
		repo_name: &str,
		pull_number: i64,
	) -> Result<github::PullRequest> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
				base_url = github::base_api_url(),
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
				base_url = github::base_api_url(),
				owner = owner,
				repo = repo_name,
				head = head,
			))
			.await
			.map(|v| v.first().cloned())
	}

	/// Creates a new pull request to merge `head` into `base`.
	pub async fn create_pull_request<A>(
		&self,
		owner: &str,
		repo_name: A,
		title: A,
		body: A,
		head: A,
		base: A,
	) -> Result<github::PullRequest>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name.as_ref(),
		);
		let params = serde_json::json!({
			"title": title.as_ref(),
			"body": body.as_ref(),
			"head": head.as_ref(),
			"base": base.as_ref(),
		});
		self.client
			.post_response(&url, &params)
			.await?
			.json()
			.await
			.context(error::Http)
	}

	/// Merges a pull request.
	pub async fn merge_pull_request(
		&self,
		owner: &str,
		repo_name: &str,
		number: i64,
		head_sha: &str,
	) -> Result<reqwest::Response> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{number}/merge",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name,
			number = number,
		);
		let params = serde_json::json!({
			"sha": head_sha,
			"merge_method": "squash"
		});
		self.client.put_response(&url, &params).await
	}

	/// Closes a pull request.
	pub async fn close_pull_request<A>(
		&self,
		owner: &str,
		repo_name: A,
		pull_number: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name.as_ref(),
			pull_number = pull_number
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
	fn test_get_pr() {
		dotenv::dotenv().ok();

		let installation = dotenv::var("TEST_INSTALLATION_LOGIN")
			.expect("TEST_INSTALLATION_LOGIN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			let _ = dbg!(
				github_bot
					.pull_request("paritytech", "substrate", 6276)
					.await
			);
		});
	}
}
