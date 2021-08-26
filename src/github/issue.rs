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
			number,
			comment: body,
		} = args;
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{number}/comments",
			base = self.base_url,
			owner = owner,
			repo = repo_name,
			number = number
		);
		self.client
			.post_response(&url, &serde_json::json!({ "body": body }))
			.await
			.map(|_| ())
	}
}
