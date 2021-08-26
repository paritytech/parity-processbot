use super::*;
use crate::{error::*, types::*};

impl Bot {
	pub async fn pull_request<'a>(
		&self,
		args: PullRequestArgs<'a>,
	) -> Result<PullRequest> {
		let PullRequestArgs {
			owner,
			repo_name,
			pull_number,
		} = args;
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
				base_url = self.base_url,
				owner = owner,
				repo = repo_name,
				pull_number = pull_number
			))
			.await
	}

	pub async fn merge_pull_request<'a>(
		&self,
		args: MergePullRequestArgs<'a>,
	) -> Result<()> {
		let MergePullRequestArgs {
			owner,
			repo_name,
			number,
			head_sha,
		} = args;
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{number}/merge",
			base_url = self.base_url,
			owner = owner,
			repo = repo_name,
			number = number,
		);
		let params = serde_json::json!({
			"sha": head_sha,
			"merge_method": "squash"
		});
		self.client.put_response(&url, &params).await.map(|_| ())
	}

	pub async fn approve_merge_request(
		&self,
		args: ApproveMergeRequestArgs<'a>,
	) -> Result<Review> {
		let ApproveMergeRequestArgs {
			owner,
			repo_name,
			pr_number,
		} = args;
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews",
			self.base_url, owner, repo_name, pr_number
		);
		let body = &serde_json::json!({ "event": "APPROVE" });
		self.client.post(url, body).await
	}

	pub async fn clear_bot_approval<'a>(
		&self,
		args: ClearBotApprovalArgs<'a>,
	) -> Result<Review> {
		let ClearBotApprovalArgs {
			owner,
			repo_name,
			pr_number,
			review_id,
		} = args;
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews/{}/dismissals",
			self.base_url, owner, repo_name, pr_number, review_id
		);
		let body = &serde_json::json!({
			"message": "Merge failed despite bot approval, therefore the approval will be dismissed."
		});
		self.client.put(url, body).await
	}
}
