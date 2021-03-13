use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	/// Returns a repository with the given name.
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
			base_url = github::base_api_url(),
			owner = owner,
			repo_name = repo_name
		);
		self.client.get(url).await
	}
}

/*
#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_repositories() {
		dotenv::dotenv().ok();

		let installation = dotenv::var("TEST_INSTALLATION_LOGIN")
			.expect("TEST_INSTALLATION_LOGIN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");
		let test_repo_name =
			dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			assert!(github_bot.repository(&test_repo_name).await.is_ok());
		});
	}
}
*/
