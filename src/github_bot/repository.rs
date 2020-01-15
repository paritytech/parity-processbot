use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	/// Returns all of the repositories managed by the organization.
	pub async fn repositories(&self) -> Result<Vec<github::Repository>> {
		self.client.get_all(&self.organization.repos_url).await
	}

	/// Returns a repository with the given name.
	pub async fn repository<A>(
		&self,
		repo_name: A,
	) -> Result<github::Repository>
	where
		A: std::fmt::Display,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo_name}",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo_name = repo_name
		);
		self.client.get(url).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_repositories() {
		dotenv::dotenv().ok();

		let github_organization =
			dotenv::var("GITHUB_ORGANIZATION").expect("GITHUB_ORGANIZATION");
		let github_token = dotenv::var("GITHUB_TOKEN").expect("GITHUB_TOKEN");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot =
				GithubBot::new(&github_organization, &github_token)
					.await
					.expect("github_bot");
			assert!(github_bot.repository("parity-processbot").await.is_ok());
			assert!(github_bot
				.repositories()
				.await
				.expect("repositories")
				.iter()
				.any(|repo| repo
					.name == 
					"parity-processbot"));
		});
	}
}
