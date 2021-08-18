use crate::{error, github, Result};

use snafu::{OptionExt, ResultExt};

use super::GithubBot;

use regex::Regex;

impl GithubBot {
	/// Adds a comment to an issue.
	pub async fn create_issue_comment(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: usize,
		comment: &str,
	) -> Result<()> {
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_number}/comments",
			base = self.base_url,
			owner = owner,
			repo = repo_name,
			issue_number = issue_number
		);
		self.client
			.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}
}
