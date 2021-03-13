use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	/// Returns a tag in a repository.
	pub async fn tag(
		&self,
		owner: &str,
		repo_name: &str,
		tag_name: &str,
	) -> Result<github::Ref> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/git/ref/tags/{tag_name}",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name,
			tag_name = tag_name,
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
	fn test_tag() {
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
			let tag = dbg!(github_bot
				.tag(&test_repo_name, "v0.1.0")
				.await
				.expect("tag"));
		});
	}
}
*/
