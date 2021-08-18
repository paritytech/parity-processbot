use crate::{error::Error, github_bot::GithubBot, Result};

// This is a lame alternative to an async closure.
pub struct GithubUserAuthenticator {
	username: String,
	org: String,
	repo_name: String,
	pr_number: usize,
}

impl GithubUserAuthenticator {
	pub fn new(
		username: &str,
		org: &str,
		repo_name: &str,
		pr_number: usize,
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
		github_bot.org_membership(&self.org, &self.username)
	}
}
