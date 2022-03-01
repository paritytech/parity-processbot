use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn team_members(
		&self,
		org: &str,
		team: &str,
	) -> Result<Vec<github::User>> {
		self.client
			.get_all(format!(
				"{}/orgs/{}/teams/{}/members",
				self.github_api_url, org, team,
			))
			.await
	}
}
