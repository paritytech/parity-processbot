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

	pub fn organization_login(&self) -> &str {
		self.organization.login.as_ref()
	}

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
	pub async fn new(
		private_key: impl Into<Vec<u8>>,
		installation_login: &str,
	) -> Result<Self> {
		let client = crate::http::Client::new(private_key.into());

		let installations = Self::installations(&client).await?;
		let installation = installations
			.iter()
			.find(|installation| {
				installation.account.login == installation_login
			})
			.context(error::MissingData)?;

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

	/// Returns check runs associated with a pull request.
	pub async fn check_runs(
		&self,
		repo_name: &str,
		pull_request: &github::PullRequest,
	) -> Result<github::CheckRuns> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
			sha = &pull_request.head.sha
		);
		self.client.get(url).await
	}

	/// Returns statuses associated with a pull request.
	pub async fn status(
		&self,
		repo_name: &str,
		pull_request: &github::PullRequest,
	) -> Result<github::CombinedStatus> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
			sha = &pull_request.head.sha
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

	/// Returns all commits since a given date.
	pub async fn commits(
		&self,
		repo_name: &str,
		sha: &str,
		since: chrono::DateTime<chrono::Utc>,
	) -> Result<Vec<github::Commit>> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
		);
		let body =
			serde_json::json!({ "sha": sha, "since": since.to_rfc3339() });
		self.client.get_with_params(url, body).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_statuses() {
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
			let status = github_bot
				.status(&test_repo_name, &created_pr)
				.await
				.expect("statuses");
			assert!(status.state != github::StatusState::Failure);
			github_bot
				.close_pull_request(&test_repo_name, created_pr.number)
				.await
				.expect("close_pull_request");
		});
	}

	#[ignore]
	#[test]
	fn test_contents() {
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
			let _contents = github_bot
				.contents(&test_repo_name, "README.md")
				.await
				.expect("contents");
		});
	}
}
