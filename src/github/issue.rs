use super::*;
use crate::{error::*, types::*};

impl Bot {
	pub async fn create_issue_comment<'a>(
		&self,
		args: CreateIssueCommentArgs<'a>,
	) -> Result<()> {
		let CreateIssueCommentArgs {
			owner,
			repo_name,
			issue_number,
			comment,
		} = args;
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
