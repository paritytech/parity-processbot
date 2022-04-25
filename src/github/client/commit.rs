use super::GithubClient;
use crate::{github::*, types::Result};

impl GithubClient {
	pub async fn statuses(
		&self,
		owner: &str,
		repo: &str,
		sha: &str,
	) -> Result<Vec<GithubCommitStatus>> {
		let mut page = 1;
		const PER_PAGE_MAX: usize = 100;

		let mut statuses = vec![];
		loop {
			let url = format!(
				"{}/repos/{}/{}/statuses/{}?per_page={}&page={}",
				self.github_api_url, owner, repo, sha, PER_PAGE_MAX, page
			);
			let page_statuses = self
				.client
				.get::<String, Vec<GithubCommitStatus>>(url)
				.await?;

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
	) -> Result<Vec<GithubCheckRun>> {
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
}
