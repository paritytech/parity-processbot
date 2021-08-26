use super::*;
use crate::{error::*, types::*};

impl Bot {
	pub async fn status<'a>(
		&self,
		args: StatusArgs<'a>,
	) -> Result<CombinedStatus> {
		let StatusArgs {
			owner,
			repo_name,
			sha,
		} = args;
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = self.base_url,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	pub async fn check_runs<'a>(
		&self,
		args: StatusArgs<'a>,
	) -> Result<CheckRuns> {
		let StatusArgs {
			owner,
			repo_name,
			sha,
		} = args;
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
