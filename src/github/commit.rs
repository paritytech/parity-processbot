use crate::{crate::github::Bot, types::Result};

impl Bot {
	/// Returns statuses for a reference.
	pub async fn status(
		&self,
		owner: &str,
		repo_name: &str,
		sha: &str,
	) -> Result<CombinedStatus> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = self.base_url,
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
	) -> Result<CheckRuns> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs",
			base_url = self.base_url,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}
}
