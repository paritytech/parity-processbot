use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn team(&self, owner: &str, slug: &str) -> Result<github::Team> {
		let url = format!(
			"{base_url}/orgs/{owner}/teams/{slug}",
			base_url = self.base_url,
			owner = owner,
			slug = slug
		);
		self.client.get(url).await
	}

	pub async fn team_members(
		&self,
		team_id: usize,
	) -> Result<Vec<github::User>> {
		self.client
			.get_all(format!("{}/teams/{}/members", self.base_url, team_id))
			.await
	}

	pub async fn core_devs(&self, owner: &str) -> Result<Vec<User>> {
		self.team(owner, CORE_DEVS_GROUP)
			.and_then(|team| self.team_members(team.id))
			.await
	}

	pub async fn substrate_team_leads(&self, owner: &str) -> Result<Vec<User>> {
		self.team(owner, SUBSTRATE_TEAM_LEADS_GROUP)
			.and_then(|team| self.team_members(team.id))
			.await
	}
}
