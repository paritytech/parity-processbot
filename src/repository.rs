use crate::{error, github, github_bot::GithubBot, Result};

impl github::Repository {
	pub fn project_owner(&self) -> Option<github::User> {
                unimplemented!();
        }
	pub fn delegated_reviewer(&self) -> Option<github::User> {
                unimplemented!();
        }
	pub fn whitelist(&self) -> Vec<github::User> {
                unimplemented!();
        }
}
