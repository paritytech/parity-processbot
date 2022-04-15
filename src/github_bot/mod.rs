use crate::{config::MainConfig, github::*, Result};

pub mod issue;
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

	pub async fn statuses(
		&self,
		owner: &str,
		repo: &str,
		sha: &str,
	) -> Result<Vec<Status>> {
		let mut page = 1;
		const PER_PAGE_MAX: usize = 100;

		let mut statuses = vec![];
		loop {
			let url = format!(
				"{}/repos/{}/{}/statuses/{}?per_page={}&page={}",
				self.github_api_url, owner, repo, sha, PER_PAGE_MAX, page
			);
			let page_statuses =
				self.client.get::<String, Vec<Status>>(url).await?;

			let should_break = page_statuses.len() < PER_PAGE_MAX;

			statuses.extend(page_statuses);

			if should_break {
				break;
			}

			page += 1;
		}

		Ok(statuses)
	}

	pub async fn check_runs(
		&self,
		owner: &str,
		repo: &str,
		sha: &str,
	) -> Result<Vec<CheckRun>> {
		let mut page = 1;
		const PER_PAGE_MAX: usize = 100;

		let mut check_runs = vec![];
		loop {
			let url = format!(
				"{}/repos/{}/{}/commits/{}/check-runs?per_page={}&page={}",
				self.github_api_url, owner, repo, sha, PER_PAGE_MAX, page
			);

			let page_check_runs =
				self.client.get::<String, CheckRuns>(url).await?;

			let should_break = page_check_runs.check_runs.len() < PER_PAGE_MAX;

			check_runs.extend(page_check_runs.check_runs);

			if should_break {
				break;
			}

			page += 1;
		}

		Ok(check_runs)
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
