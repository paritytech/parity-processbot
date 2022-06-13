use super::GithubClient;
use crate::types::Result;

impl GithubClient {
	pub async fn create_issue_comment(
		&self,
		owner: &str,
		repo: &str,
		number: i64,
		comment: &str,
	) -> Result<()> {
		let url = format!(
			"{}/repos/{}/{}/issues/{}/comments",
			self.github_api_url, owner, repo, number
		);
		self.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}

	pub async fn acknowledge_issue_comment(
		&self,
		owner: &str,
		repo: &str,
		comment_id: i64,
	) -> Result<()> {
		let url = format!(
			"{}/repos/{}/{}/issues/comments/{}/reactions",
			self.github_api_url, owner, repo, comment_id
		);
		self.post_response(&url, &serde_json::json!({ "content": "+1" }))
			.await
			.map(|_| ())
	}
}
