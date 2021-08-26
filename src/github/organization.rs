use super::*;

use crate::error::*;

impl Bot {
	pub async fn org_membership<'a>(
		&self,
		args: OrgMembershipArgs<'a>,
	) -> Result<bool> {
		let OrgMembershipArgs { org, username } = args;
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
