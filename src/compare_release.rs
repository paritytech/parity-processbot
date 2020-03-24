use crate::{bots, constants::*, github, Result};

impl bots::Bot {
	pub async fn compare_release_if_requested(
		&self,
		repo_name: &str,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		let comments = self
			.github_bot
			.get_issue_comments(repo_name, pull_request.number)
			.await?;

		let last_request = comments.iter().rev().find(|c| {
			c.body.as_ref().map_or(false, |b| {
				b.to_lowercase().trim()
					== COMPARE_RELEASE_REQUEST.to_lowercase().trim()
			})
		});

		let last_compare = comments.iter().rev().find(|c| {
			c.user.login == BOT_GITHUB_LOGIN
				&& c.body.as_ref().map_or(false, |b| {
					b.to_lowercase()
						.trim()
						.contains(COMPARE_RELEASE_REPLY.to_lowercase().trim())
				})
		});

		if last_request.map(|x| x.created_at)
			> last_compare.map(|x| x.created_at)
		{
			let rel = self.github_bot.latest_release(repo_name).await?;
			let rel_tag = self.github_bot.tag(repo_name, &rel.tag_name).await?;
			let link = self.github_bot.diff_url(
				repo_name,
				&rel_tag.object.sha,
				&pull_request.head.sha,
			);
			self.github_bot
				.create_issue_comment(
					&repo_name,
					pull_request.number,
					&format!("{} {}", COMPARE_RELEASE_REPLY, link),
				)
				.await?;
		}

		Ok(())
	}
}
