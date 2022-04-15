use crate::Result;

use super::GithubBot;

impl GithubBot {
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
		self.client
			.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}
}
