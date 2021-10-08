use crate::{github_bot::GithubBot, Result};

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
		match github_bot.org_member(&self.org, &self.username).await {
			Ok(_) => Ok(()),
			Err(e) => Err(e.map_issue((
				self.org.clone(),
				self.repo_name.clone(),
				self.pr_number,
			))),
		}
	}
}
