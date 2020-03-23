use crate::{error, github, Result};

use snafu::OptionExt;

use super::GithubBot;

impl GithubBot {
	/// Returns the latest release in a repository.
	pub fn diff_url(&self, repo_name: &str, base: &str, head: &str) -> String {
		format!(
			"{base_url}/{owner}/{repo}/compare/{base}...{head}",
			base_url = Self::HTML_BASE_URL,
			owner = self.organization.login,
			repo = repo_name,
			base = base,
			head = head,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_diff_url() {
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
			let diff = dbg!(github_bot.diff_url(
				&test_repo_name,
				"d383b0dd542bc04d6fd7042205f353cfc76d5502",
				"68e51d6e24862d499b5f042321cc87b172579e74",
			));
		});
	}
}
