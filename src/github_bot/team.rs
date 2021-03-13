use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	/// Returns the team with a given team slug (eg. 'core-devs').
	pub async fn team(&self, owner: &str, slug: &str) -> Result<github::Team> {
		let url = format!(
			"{base_url}/orgs/{owner}/teams/{slug}",
			base_url = github::base_api_url(),
			owner = owner,
			slug = slug
		);
		self.client.get(url).await
	}

	/// Returns members of the team with a id.
	pub async fn team_members(
		&self,
		team_id: i64,
	) -> Result<Vec<github::User>> {
		self.client
			.get_all(format!(
				"{}/teams/{}/members",
				github::base_api_url(),
				team_id
			))
			.await
	}
}

/*
#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_teams() {
		dotenv::dotenv().ok();

		let installation = dotenv::var("TEST_INSTALLATION_LOGIN")
			.expect("TEST_INSTALLATION_LOGIN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			let team = github_bot.team("core-devs").await.expect("team");
			let _members = github_bot
				.team_members(team.id)
				.await
				.expect("team members");
		});
	}
}
*/
