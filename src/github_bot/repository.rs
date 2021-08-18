use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn repository<A>(
		&self,
		owner: &str,
		repo_name: A,
	) -> Result<github::Repository>
	where
		A: std::fmt::Display,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo_name}",
			base_url = self.base_url,
			owner = owner,
			repo_name = repo_name
		);
		self.client.get(url).await
	}
}
