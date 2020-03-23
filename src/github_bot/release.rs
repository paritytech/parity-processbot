use crate::{error, github, Result};

use snafu::OptionExt;

use super::GithubBot;

impl GithubBot {
	/// Returns the latest release in a repository.
	pub async fn latest_release(
		&self,
		repo_name: &str,
	) -> Result<github::Release> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/releases/latest",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
		);
		self.client.get(url).await
	}

    /*
	/// Returns the assets associated with a release.
	pub async fn release_assets(
		&self,
		repo_name: &str,
		release_id: i64,
	) -> Result<Vec<github::Asset>> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/releases/{release_id}/assets",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
			release_id = release_id,
		);
		self.client.get(url).await
	}
    */
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_release() {
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
			let release = dbg!(github_bot
				.latest_release(&test_repo_name)
				.await
				.expect("release"));
//			let assets = dbg!(github_bot
//				.release_assets(&test_repo_name, release.id,)
//				.await
//				.expect("release assets"));
			assert_eq!(release.tag_name, "v0.1.0".to_owned(),);
		});
	}
}
