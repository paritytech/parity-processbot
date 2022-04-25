use crate::Result;

use super::GithubBot;

impl GithubBot {
	pub async fn org_member(&self, org: &str, username: &str) -> Result<bool> {
		let url = &format!(
			"{}/orgs/{}/members/{}",
			self.github_api_url, org, username
		);
		let status = self.client.get_status(url).await?;
		// https://docs.github.com/en/rest/orgs/members#check-organization-membership-for-a-user--code-samples
		Ok(status == 204)
	}
}