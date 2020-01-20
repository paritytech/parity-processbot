use crate::{error, github, Result};

use snafu::OptionExt;

use super::GithubBot;

impl GithubBot {
	/// Returns all reviews associated with a pull request.
	pub async fn reviews(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Vec<github::Review>> {
		let url = format!(
			"{}/reviews",
			pull_request.url.as_ref().context(error::MissingData)?
		);
		self.client.get_all(url).await
	}

	/// Returns all review requests associated with a pull request.
	pub async fn requested_reviewers(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<github::RequestedReviewers> {
		let url = format!(
			"{}/requested_reviewers",
			pull_request.url.as_ref().context(error::MissingData)?
		);
		self.client.get(url).await
	}

	/// Requests reviews from users.
	pub async fn request_reviews(
		&self,
		pull_request: &github::PullRequest,
		reviewers: &[&str],
	) -> Result<github::PullRequest> {
		let url = format!(
			"{}/requested_reviewers",
			pull_request.url.as_ref().context(error::MissingData)?
		);
		let body = &serde_json::json!({ "reviewers": reviewers });
		self.client.post(url, body).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_reviews() {
		dotenv::dotenv().ok();

		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot =
				GithubBot::new(private_key).await.expect("github_bot");
			let created = github_bot
				.create_pull_request(
					"parity-processbot",
					"testing pr",
					"this is a test",
					"testing_branch",
					"other_testing_branch",
				)
				.await
				.expect("create_pull_request");
			github_bot
				.request_reviews(&created, &["folsen"])
				.await
				.expect("request_reviews");
			let requested_reviewers = github_bot
				.requested_reviewers(&created)
				.await
				.expect("requested_reviewers");
			assert!(requested_reviewers
				.users
				.iter()
				.any(|x| x.login == "folsen"));
			github_bot.reviews(&created).await.expect("reviews");
			github_bot
				.close_pull_request(
					"parity-processbot",
					created.number.expect("created pr number"),
				)
				.await
				.expect("close_pull_request");
		});
	}
}
