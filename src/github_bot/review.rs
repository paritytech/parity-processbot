use crate::{github, Result};

use super::GithubBot;

impl GithubBot {
	/// Returns all reviews associated with a pull request.
	pub async fn reviews(&self, pr_url: &str) -> Result<Vec<github::Review>> {
		let url = format!("{}/reviews", pr_url);
		self.client.get_all(url).await
	}

	/// Returns all review requests associated with a pull request.
	pub async fn requested_reviewers(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<github::RequestedReviewers> {
		let url = format!("{}/requested_reviewers", pull_request.url);
		self.client.get(url).await
	}

	/// Requests reviews from users.
	pub async fn request_reviews(
		&self,
		pull_request: &github::PullRequest,
		reviewers: &[&str],
	) -> Result<github::PullRequest> {
		let url = format!("{}/requested_reviewers", pull_request.url);
		let body = &serde_json::json!({ "reviewers": reviewers });
		self.client.post(url, body).await
	}
}

/*
#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_reviews() {
		dotenv::dotenv().ok();

		let installation = dotenv::var("TEST_INSTALLATION_LOGIN")
			.expect("TEST_INSTALLATION_LOGIN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");
		let test_repo_name =
			dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot = GithubBot::new(private_key, &installation)
				.await
				.expect("github_bot");
			let created = github_bot
				.create_pull_request(
					&test_repo_name,
					&"testing pr".to_owned(),
					&"this is a test".to_owned(),
					&"testing_branch".to_owned(),
					&"other_testing_branch".to_owned(),
				)
				.await
				.expect("create_pull_request");
			github_bot
				.request_reviews(&created, &["sjeohp"])
				.await
				.expect("request_reviews");
			let requested_reviewers = github_bot
				.requested_reviewers(&created)
				.await
				.expect("requested_reviewers");
			assert!(requested_reviewers
				.users
				.iter()
				.any(|x| x.login == "sjeohp"));
			github_bot.reviews(&created.url).await.expect("reviews");
			github_bot
				.close_pull_request(&test_repo_name, created.number)
				.await
				.expect("close_pull_request");
		});
	}
}
*/
