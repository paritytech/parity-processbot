use crate::{config::MainConfig, constants::*, github::*, Result};
use futures_util::TryFutureExt;

pub mod issue;
pub mod project;
pub mod pull_request;
pub mod review;
pub mod team;

pub struct GithubBot {
	pub client: crate::http::Client,
	github_api_url: String,
}

impl GithubBot {
	pub fn new(config: &MainConfig) -> Self {
		let client = crate::http::Client::new(config);

		Self {
			client,
			github_api_url: config.github_api_url.clone(),
		}
	}

	pub async fn status(
		&self,
		owner: &str,
		repo: &str,
		sha: &str,
	) -> Result<CombinedStatus> {
		let url = format!(
			"{}/repos/{}/{}/commits/{}/status",
			self.github_api_url, owner, repo, sha
		);
		self.client.get(url).await
	}

	pub async fn check_runs(
		&self,
		owner: &str,
		repo: &str,
		sha: &str,
	) -> Result<CheckRuns> {
		let url = format!(
			"{}/repos/{}/{}/commits/{}/check-runs",
			self.github_api_url, owner, repo, sha
		);
		self.client.get(url).await
	}

	pub async fn contents(
		&self,
		owner: &str,
		repo: &str,
		path: &str,
		ref_field: &str,
	) -> Result<Contents> {
		let url = &format!(
			"{}/repos/{}/{}/contents/{}?ref={}",
			self.github_api_url, owner, repo, path, ref_field
		);
		self.client.get(url).await
	}

	pub async fn org_member(&self, org: &str, username: &str) -> Result<bool> {
		let url = &format!(
			"{}/orgs/{}/members/{}",
			self.github_api_url, org, username
		);
		let status = self.client.get_status(url).await?;
		Ok(status == 204) // Github API returns HTTP 204 (No Content) if the user is a member
	}

	pub async fn approve_merge_request(
		&self,
		owner: &str,
		repo: &str,
		pr_number: i64,
	) -> Result<Review> {
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews",
			self.github_api_url, owner, repo, pr_number
		);
		let body = &serde_json::json!({ "event": "APPROVE" });
		self.client.post(url, body).await
	}

	pub async fn clear_merge_request_approval(
		&self,
		owner: &str,
		repo: &str,
		pr_number: i64,
		review_id: i64,
	) -> Result<Review> {
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews/{}/dismissals",
			self.github_api_url, owner, repo, pr_number, review_id
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
