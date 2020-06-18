use crate::{github, Result};

pub mod issue;
pub mod project;
pub mod pull_request;
pub mod release;
pub mod repository;
pub mod review;
pub mod tag;
pub mod team;

pub struct GithubBot {
	client: crate::http::Client,
}

impl GithubBot {
	pub(crate) const BASE_URL: &'static str = "https://api.github.com";
	pub(crate) const BASE_HTML_URL: &'static str = "https://github.com";

	/// Creates a new instance of `GithubBot` from a GitHub organization defined
	/// by `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organization does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub async fn new(
		private_key: impl Into<Vec<u8>>,
		installation_login: &str,
	) -> Result<Self> {
		let client = crate::http::Client::new(
			private_key.into(),
			installation_login.to_owned(),
		);

		Ok(Self { client })
	}

	pub fn owner_from_html_url(url: &str) -> Option<&str> {
		url.split("/").skip(3).next()
	}

	pub async fn installation_repositories(
		&self,
	) -> Result<github::InstallationRepositories> {
		self.client
			.get(&format!("{}/installation/repositories", Self::BASE_URL))
			.await
	}

	/// Returns statuses for a reference.
	pub async fn status(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<github::CombinedStatus> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	/// Returns check runs associated for a reference.
	pub async fn check_runs(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<github::CheckRuns> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	/// Returns the contents of a file in a repository.
	pub async fn contents(
		&self,
		owner: &str,
		repo_name: &str,
		path: &str,
		ref_field: &str,
	) -> Result<github::Contents> {
		let url = &format!(
			"{base_url}/repos/{owner}/{repo_name}/contents/{path}?ref={ref_field}",
			base_url = Self::BASE_URL,
			owner = owner,
			repo_name = repo_name,
			path = path,
            ref_field = ref_field
		);
		self.client.get(url).await
	}

	/// Returns a link to a diff.
	pub fn diff_url(
		&self,
		owner: &str,
		repo_name: &str,
		base: &str,
		head: &str,
	) -> String {
		format!(
			"{base_url}/{owner}/{repo}/compare/{base}...{head}",
			base_url = Self::BASE_HTML_URL,
			owner = owner,
			repo = repo_name,
			base = base,
			head = head,
		)
	}

	/// Returns true if the user is a member of the org.
	pub async fn org_member(&self, org: &str, username: &str) -> Result<u16> {
		let url = &format!(
			"{base_url}/orgs/{org}/members/{username}",
			base_url = Self::BASE_URL,
			org = org,
			username = username,
		);
		self.client.get_status(url).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_org_member() {
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
			let member = dbg!(github_bot
				.org_member(&installation, "sjeohp")
				.await
				.expect("org_member"));
		});
	}
	/*
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
					.status(&test_repo_name, &created_pr.head.sha)
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
	*/
}
