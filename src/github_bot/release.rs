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
			base_url = Self::BASE_URL,
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
