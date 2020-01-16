use crate::{error, github, Result};

use snafu::OptionExt;

pub mod issue;
pub mod project;
pub mod pull_request;
pub mod repository;
pub mod review;
pub mod team;

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

	/// Returns statuses associated with a pull request.
	pub async fn statuses(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Vec<github::Status>> {
		self.client
			.get(pull_request.statuses_url.as_ref().context(error::MissingData)?)
			.await
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
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_statuses() {
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
			let created_pr = github_bot
				.create_pull_request(
					"parity-processbot",
					"testing pr",
					"this is a test",
					"testing_branch",
					"other_testing_branch",
				)
				.await
				.expect("create_pull_request");
			let statuses =
				github_bot.statuses(&created_pr).await.expect("statuses");
			assert!(statuses.len() > 0);
			github_bot
				.close_pull_request(
					"parity-processbot",
					created_pr.number.expect("created pr number"),
				)
				.await
				.expect("close_pull_request");
		});
	}

	#[ignore]
	#[test]
	fn test_contents() {
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
			let _contents = github_bot
				.contents("parity-processbot", "README.md")
				.await
				.expect("contents");
		});
	}
}
