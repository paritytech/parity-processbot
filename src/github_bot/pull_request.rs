use crate::{error, github, Result};

use snafu::ResultExt;

use super::GithubBot;

impl GithubBot {
	pub async fn create_pull_request<A>(
		&self,
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
			base_url = Self::BASE_URL,
			owner = self.organization.login,
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

	pub async fn merge_pull_request<A>(
		&self,
		repo_name: A,
		pull_number: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}/merge",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name.as_ref(),
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
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
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

	#[test]
	fn test_pull_requests() {
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
				.create_pull_request(
					"parity-processbot",
					"testing pr",
					"this is a test",
					"testing_branch",
					"other_testing_branch",
				)
				.await
				.expect("create_pull_request");
			let prs = github_bot
				.pull_requests(&repo)
				.await
				.expect("pull_requests");
			assert!(prs.iter().any(|pr| pr.title == "testing pr"));
			github_bot
				.close_pull_request(
					"parity-processbot",
					created.number.expect("created pr id"),
				)
				.await
				.expect("close_pull_request");
			let prs = github_bot
				.pull_requests(&repo)
				.await
				.expect("pull_requests");
			assert!(!prs.iter().any(|pr| pr.title == "testing pr"));
		});
	}
}
