use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn team(&self, owner: &str, slug: &str) -> Result<github::Team> {
		let url =
			format!("{}/orgs/{}/teams/{}", self.github_api_url, owner, slug);
		self.client.get(url).await
	}

	pub async fn team_members(
		&self,
		team_id: i64,
	) -> Result<Vec<github::User>> {
		self.client
			.get_all(format!(
				"{}/teams/{}/members",
				self.github_api_url, team_id,
			))
			.await
	}
}
