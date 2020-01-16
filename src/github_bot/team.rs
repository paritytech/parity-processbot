use crate::{error, github, Result};

use snafu::OptionExt;

use super::GithubBot;

impl GithubBot {
	/// Returns the team with a given team slug (eg. 'core-devs').
	pub async fn team(&self, slug: &str) -> Result<github::Team> {
		let url = self.organization.url.as_ref().context(error::MissingData)?;
		self.client.get(format!("{}/teams/{}", url, slug)).await
	}

	/// Returns members of the team with a id.
	pub async fn team_members(
		&self,
		team_id: i64,
	) -> Result<Vec<github::User>> {
		self.client
			.get(format!("{}/teams/{}/members", Self::BASE_URL, team_id))
			.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_teams() {
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
			let team = github_bot.team("core-devs").await.expect("team");
			let members = github_bot
				.team_members(team.id.expect("team id"))
				.await
				.expect("team members");
			assert!(members.len() > 0);
		});
	}
}
