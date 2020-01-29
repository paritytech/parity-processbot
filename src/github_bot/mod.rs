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
	pub(crate) const BASE_URL: &'static str = "https://api.github.com";

	/// This method doesn't use `self`, as we need to use it before we
	/// initialise `Self`.
	async fn installations(
		client: &crate::http::Client,
	) -> Result<Vec<github::Installation>> {
		client
			.jwt_get(&format!("{}/app/installations", Self::BASE_URL))
			.await
	}

	/// Creates a new instance of `GithubBot` from a GitHub organization defined
	/// by `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organization does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub async fn new(private_key: impl Into<Vec<u8>>) -> Result<Self> {
		let client = crate::http::Client::new(private_key.into());

		let installations = Self::installations(&client).await?;
		let installation = installations.first().context(error::MissingData)?;

		let organization = client
			.get(&format!(
				"{}/orgs/{}",
				Self::BASE_URL,
				&installation.account.login
			))
			.await?;

		Ok(Self {
			client,
			organization,
		})
	}

	/// Returns statuses associated with a pull request.
	pub async fn status(
		&self,
		repo_name: &str,
		pull_request: &github::PullRequest,
	) -> Result<github::Status> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
			sha = &pull_request
				.merge_commit_sha
				.as_ref()
				.context(error::MissingData)?,
		);
		self.client.get(url).await
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
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");
		let test_repo_name =
			dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot =
				GithubBot::new(private_key).await.expect("github_bot");
			let created_pr = github_bot
				.create_pull_request(
					&test_repo_name,
					&"testing pr".to_owned(),
					&"this is a test".to_owned(),
					&"testing_branch".to_owned(),
					&"other_testing_branch".to_owned(),
				)
				.await
				.expect("create_pull_request");
			let status = dbg!(github_bot
				.status(&test_repo_name, &created_pr)
				.await
				.expect("statuses"));
			assert!(status.state != github::StatusState::Failure);
			github_bot
				.close_pull_request(
					&test_repo_name,
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
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");
		let test_repo_name =
			dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");
		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot =
				GithubBot::new(private_key).await.expect("github_bot");
			let _contents = github_bot
				.contents(&test_repo_name, "README.md")
				.await
				.expect("contents");
		});
	}
}
