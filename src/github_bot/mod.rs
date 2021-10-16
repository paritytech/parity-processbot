use crate::{constants::*, github::*, Result};
use futures_util::TryFutureExt;

pub mod issue;
pub mod project;
pub mod pull_request;
pub mod review;
pub mod team;

pub struct GithubBot {
	pub client: crate::http::Client,
}

impl GithubBot {
	pub(crate) const BASE_URL: &'static str = "https://api.github.com";

	pub fn new(
		private_key: impl Into<Vec<u8>>,
		installation_login: &str,
		github_app_id: usize,
	) -> Result<Self> {
		let client = crate::http::Client::new(
			private_key.into(),
			installation_login.to_owned(),
			github_app_id,
		);

		Ok(Self { client })
	}

	pub async fn installation_repositories(
		&self,
	) -> Result<InstallationRepositories> {
		self.client
			.get(&format!("{}/installation/repositories", Self::BASE_URL))
			.await
	}

	pub async fn status(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<CombinedStatus> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	pub async fn check_runs(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<CheckRuns> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	pub async fn contents(
		&self,
		owner: &str,
		repo_name: &str,
		path: &str,
		ref_field: &str,
	) -> Result<Contents> {
		let url = &format!(
			"{base_url}/repos/{owner}/{repo_name}/contents/{path}?ref={ref_field}",
			base_url = Self::BASE_URL,
			owner = owner,
			repo_name = repo_name,
			path = path,
			ref_field = ref_field,
		);
		self.client.get(url).await
	}

	pub async fn org_member(&self, org: &str, username: &str) -> Result<bool> {
		let url = &format!(
			"{base_url}/orgs/{org}/members/{username}",
			base_url = Self::BASE_URL,
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
			Self::BASE_URL,
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
			Self::BASE_URL,
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
}
