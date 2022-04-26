use super::GithubClient;
use crate::{github::*, types::Result};

impl GithubClient {
	pub async fn contents(
		&self,
		owner: &str,
		repo: &str,
		path: &str,
		ref_field: &str,
	) -> Result<GithubFileContents> {
		let url = &format!(
			"{}/repos/{}/{}/contents/{}?ref={}",
			self.github_api_url, owner, repo, path, ref_field
		);
		self.get(url).await
	}
}
