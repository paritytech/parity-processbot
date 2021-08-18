use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	pub async fn org_membership(
		&self,
		org: &str,
		username: &str,
	) -> Result<bool> {
		let url = &format!(
			"{base_url}/orgs/{org}/members/{username}",
			base_url = self.base_url,
			org = org,
			username = username,
		);
		let status = self.client.get_status(url).await?;
		status == 204
	}
}
