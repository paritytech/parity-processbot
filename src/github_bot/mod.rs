use crate::{config::MainConfig, github::*, Result};

pub mod issue;
pub mod project;
pub mod pull_request;

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
	) -> Result<Vec<Status>> {
		let url = format!(
			"{}/repos/{}/{}/statuses/{}",
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
}
