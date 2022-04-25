use crate::{github::*, Result};

use super::GithubBot;

impl GithubBot {
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
}
