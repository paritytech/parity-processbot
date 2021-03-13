use super::GithubBot;
use crate::{error, github, Result};
use regex::Regex;
use snafu::ResultExt;

impl GithubBot {
	/// Returns the latest release in a repository.
	pub async fn latest_release(
		&self,
		owner: &str,
		repo_name: &str,
	) -> Result<github::Release> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/releases/latest",
			base_url = github::base_api_url(),
			owner = owner,
			repo = repo_name,
		);
		self.client.get(url).await
	}

	pub async fn substrate_commit_from_polkadot_commit(
		&self,
		ref_field: &str,
	) -> Result<String> {
		let re = Regex::new(
			r"git\+https://github.com/paritytech/substrate#([0-9a-z]+)",
		)
		.expect("substrate commit regex");
		self.contents("paritytech", "polkadot", "Cargo.lock", ref_field)
			.await
			.and_then(|c| {
				base64::decode(&c.content.replace("\n", ""))
					.context(error::Base64)
			})
			.and_then(|b| String::from_utf8(b).context(error::Utf8))
			.map(|s| {
				re.captures(&s).expect("substrate in Cargo.lock")[1].to_string()
			})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_substrate_commit() {
		dotenv::dotenv().ok();
		let installation =
			dotenv::var("INSTALLATION_LOGIN").expect("INSTALLATION_LOGIN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");
		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			let commit = github_bot
				.substrate_commit_from_polkadot_commit(
					"76d6a6aa0c573c3a107e94cf954740eb84f1a092",
				)
				.await
				.unwrap();
			assert_eq!(&commit, "e7457b1eb9980596301fe1afd36478a6725157ef");
		});
	}

	/*
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
		});
	}
	*/
}
