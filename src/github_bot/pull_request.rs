use crate::{error, github, Result};

use snafu::ResultExt;

use super::GithubBot;

impl GithubBot {
	/// Returns all of the pull requests in a single repository.
	pub async fn pull_requests(
		&self,
		repo: &github::Repository,
	) -> Result<Vec<github::PullRequest>> {
		self.client
			.get_all(repo.pulls_url.replace("{/number}", ""))
			.await
	}

	/// Returns a single pull request.
	pub async fn pull_request(
		&self,
		repo: &github::Repository,
		pull_number: i64,
	) -> Result<github::PullRequest> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
				base_url = Self::BASE_URL,
				owner = self.organization.login,
				repo = repo.name,
				pull_number = pull_number
			))
			.await
	}

	/// Creates a new pull request to merge `head` into `base`.
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

	/// Merges a pull request.
	pub async fn merge_pull_request(
		&self,
		repo_name: &str,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/merges",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
		);
		self.client
			.post_response(&url, &serde_json::json!({ "base": pull_request.base.ref_field, "head": pull_request.head.sha })) 
			.await
			.map(|_| ())
	}

	/// Closes a pull request.
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

	#[ignore]
	#[test]
	fn test_pull_requests() {
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
			let repo = github_bot
				.repository(&test_repo_name)
				.await
				.expect("repository");
			let created = github_bot
				.create_pull_request(
					&test_repo_name,
					&"testing pr".to_owned(),
					&"this is a test".to_owned(),
					&"testing_branch".to_owned(),
					&"other_testing_branch".to_owned(),
				)
				.await
				.expect("create_pull_request");
			let prs = github_bot
				.pull_requests(&repo)
				.await
				.expect("pull_requests");
			assert!(prs.iter().any(|pr| pr
				.title
				.as_ref()
				.map_or(false, |x| x == "testing pr")));
			github_bot
				.close_pull_request(&test_repo_name, created.number)
				.await
				.expect("close_pull_request");
			let prs = github_bot
				.pull_requests(&repo)
				.await
				.expect("pull_requests");
			assert!(!prs.iter().any(|pr| pr
				.title
				.as_ref()
				.map_or(false, |x| x == "testing pr")));
		});
	}
}
