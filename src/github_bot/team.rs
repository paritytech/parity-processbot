use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn team(&self, owner: &str, slug: &str) -> Result<github::Team> {
		let url = format!(
			"{base_url}/orgs/{owner}/teams/{slug}",
			base_url = Self::BASE_URL,
			owner = owner,
			slug = slug
		);
		self.client.get(url).await
	}

	pub async fn team_members(
		&self,
		team_id: i64,
	) -> Result<Vec<github::User>> {
		self.client
			.get_all(format!("{}/teams/{}/members", Self::BASE_URL, team_id))
			.await
	}
}
