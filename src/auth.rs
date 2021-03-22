use crate::{error::Error, github_bot::GithubBot, Result};

// This is a lame alternative to an async closure.
pub struct GithubUserAuthenticator {
	username: String,
	org: String,
	repo_name: String,
	pr_number: i64,
}

impl GithubUserAuthenticator {
	pub fn new(
		username: &str,
		org: &str,
		repo_name: &str,
		pr_number: i64,
	) -> Self {
		Self {
			username: username.to_string(),
			org: org.to_string(),
			repo_name: repo_name.to_string(),
			pr_number,
		}
	}

	pub async fn check_org_membership(
		&self,
		github_bot: &GithubBot,
	) -> Result<()> {
		let is_member = github_bot
			.org_member(&self.org, &self.username)
			.await
			.map_err(|e| {
				Error::OrganizationMembership {
					source: Box::new(e),
				}
				.map_issue((
					self.org.clone(),
					self.repo_name.clone(),
					self.pr_number,
				))
			})?;

		if !is_member {
			Err(Error::OrganizationMembership {
				source: Box::new(Error::Message {
					msg: format!(
						"{} is not a member of {}; aborting.",
						self.username, self.org
					),
				}),
			}
			.map_issue((
				self.org.clone(),
				self.repo_name.clone(),
				self.pr_number,
			)))?;
		}
		Ok(())
	}
}
