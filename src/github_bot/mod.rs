use crate::{constants::*, github::*, Result};
use futures_util::TryFutureExt;

pub mod issue;
pub mod project;
pub mod pull_request;
pub mod release;
pub mod repository;
pub mod review;
pub mod tag;
pub mod team;

pub struct GithubBot {
	pub client: crate::http::Client,
	fetch_domain: Option<String>,
	fetch_prefix: Option<String>,
}

impl GithubBot {
	pub(crate) const BASE_HTML_URL: &'static str = "https://github.com";

	/// Creates a new instance of `GithubBot` from a GitHub organization defined
	/// by `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organization does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub async fn new(
		private_key: impl Into<Vec<u8>>,
		installation_login: &str,
		app_id: usize,
	) -> Result<Self> {
		let client = crate::http::Client::new(
			private_key.into(),
			installation_login.to_owned(),
			app_id,
		);

		Ok(Self {
			client,
			fetch_domain: None,
			fetch_prefix: None,
		})
	}

	pub fn new_for_testing(
		private_key: Vec<u8>,
		installation_login: &str,
		fetch_domain: &str,
	) -> Self {
		let client = crate::http::Client::new(
			private_key,
			installation_login.to_owned(),
			1,
		);
		Self {
			client,
			fetch_domain: Some(fetch_domain.to_string()),
			fetch_prefix: Some("".to_owned()),
		}
	}

	pub fn owner_from_html_url(url: &str) -> Option<&str> {
		url.split("/").skip(3).next()
	}

	pub async fn installation_repositories(
		&self,
	) -> Result<InstallationRepositories> {
		self.client
			.get(&format!("{}/installation/repositories", base_api_url()))
			.await
	}

	/// Returns statuses for a reference.
	pub async fn status(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<CombinedStatus> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = base_api_url(),
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		let mut result: Result<CombinedStatus> = self.client.get(url).await;
		// FIXME: stopgap hardcoded measure until vanity-service encodes which jobs
		// can be skipped into the status' description
		// https://github.com/paritytech/parity-processbot/issues/242
		if let Ok(combined) = result.as_mut() {
			combined.statuses.retain(|s| {
				s.context != "continuous-integration/gitlab-cargo-deny"
			})
		}
		result
	}

	/// Returns check runs associated for a reference.
	pub async fn check_runs(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<CheckRuns> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs",
			base_url = base_api_url(),
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
	) -> Result<Contents> {
		let url = &format!(
			"{base_url}/repos/{owner}/{repo_name}/contents/{path}?ref={ref_field}",
			base_url = base_api_url(),
			owner = owner,
			repo_name = repo_name,
			path = path,
			ref_field = ref_field,
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
	pub async fn org_member(&self, org: &str, username: &str) -> Result<bool> {
		let url = &format!(
			"{base_url}/orgs/{org}/members/{username}",
			base_url = base_api_url(),
			org = org,
			username = username,
		);
		let status = self.client.get_status(url).await?;
		Ok(status == 204) // Github API returns HTTP 204 (No Content) if the user is a member
	}

	pub async fn approve_merge_request(
		&self,
		owner: &str,
		repo_name: &str,
		pr_number: i64,
	) -> Result<Review> {
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews",
			base_api_url(),
			owner,
			repo_name,
			pr_number
		);
		let body = &serde_json::json!({ "event": "APPROVE" });
		self.client.post(url, body).await
	}

	pub async fn clear_merge_request_approval(
		&self,
		owner: &str,
		repo_name: &str,
		pr_number: i64,
		review_id: i64,
	) -> Result<Review> {
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews/{}/dismissals",
			base_api_url(),
			owner,
			repo_name,
			pr_number,
			review_id
		);
		let body = &serde_json::json!({
			"message": "Merge failed despite bot approval, therefore the approval will be dismissed."
		});
		self.client.put(url, body).await
	}

	pub async fn core_devs(&self, owner: &str) -> Result<Vec<User>> {
		self.team(owner, CORE_DEVS_GROUP)
			.and_then(|team| self.team_members(team.id))
			.await
	}

	pub async fn substrate_team_leads(&self, owner: &str) -> Result<Vec<User>> {
		self.team(owner, SUBSTRATE_TEAM_LEADS_GROUP)
			.and_then(|team| self.team_members(team.id))
			.await
	}

	pub fn get_fetch_components(
		&self,
		owner: &str,
		repository_name: &str,
		token: &str,
	) -> (String, String) {
		let prefix = self
			.fetch_prefix
			.as_ref()
			.map(|s| s.clone())
			.unwrap_or_else(|| format!("https://x-access-token:{}@", token));
		let domain = format!(
			"{}/{}/{}",
			self.fetch_domain
				.as_ref()
				.map(|s| s.clone())
				.unwrap_or_else(|| "github.com".to_string()),
			owner,
			repository_name
		);

		(format!("{}{}", &prefix, &domain), domain)
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
		let _test_repo_name =
			dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");
		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			let _member = dbg!(github_bot
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
				assert!(status.state != StatusState::Failure);
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
