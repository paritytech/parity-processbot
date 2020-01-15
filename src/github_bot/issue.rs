use crate::{error, github, Result};

use snafu::{OptionExt, ResultExt};

use super::GithubBot;

use regex::Regex;

impl GithubBot {
	/// Returns all of the issues in a single repository.
	pub async fn repository_issues(
		&self,
		repo: &github::Repository,
	) -> Result<Vec<github::Issue>> {
		self.client
			.get_all(repo.issues_url.replace("{/number}", ""))
			.await
	}

	/// Returns the issue associated with a pull request.
	pub async fn pull_request_issues(
		&self,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
	) -> Result<Vec<github::Issue>> {
		let body = pull_request.body.as_ref().context(error::MissingData)?;
		let re = Regex::new(r"#([0-9]+)").unwrap();
		Ok(futures::future::join_all(
			re.captures_iter(body)
				.filter_map(|cap| {
					cap.get(1)
						.and_then(|x| dbg!(x.as_str()).parse::<i64>().ok())
				})
				.map(|num| {
					self.client.get(format!(
						"{base_url}/repos/{owner}/{repo}/issues/{issue_number}",
						base_url = Self::BASE_URL,
						owner = self.organization.login,
						repo = &repo.name,
						issue_number = num
					))
				}),
		)
		.await
		.into_iter()
		.filter_map(|res| res.ok())
		.collect::<Vec<github::Issue>>())
	}

	/// Returns events associated with an issue.
	pub async fn issue_events(
		&self,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::IssueEvent>> {
		self.client
			.get(format!(
			"{base_url}/repos/{owner}/{repo_name}/issues/{issue_number}/events",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo_name = repo_name,
			issue_number = issue_number
		))
			.await
	}

	pub async fn create_issue<A>(
		&self,
		repo_name: A,
		title: A,
		body: A,
		assignee: A,
	) -> Result<github::Issue>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues",
			base_url = Self::BASE_URL,
			owner = self.organization.login,
			repo = repo_name.as_ref(),
		);
		let params = serde_json::json!({
						"title": title.as_ref(),
						"body": body.as_ref(),
						"assignee": assignee.as_ref(),
		});
		self.client
			.post_response(&url, &params)
			.await?
			.json()
			.await
			.context(error::Http)
	}

	/// Adds a comment to an issue.
	pub async fn create_issue_comment<A, B>(
		&self,
		repo_name: A,
		issue_number: i64,
		comment: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		log::info!("Adding comment");
		let url = format!(
			"{base}/repos/{org}/{repo}/issues/{issue_number}/comments",
			base = Self::BASE_URL,
			org = self.organization.login,
			repo = repo_name.as_ref(),
			issue_number = issue_number
		);
		log::info!("POST {}", url);
		self.client
			.post_response(
				&url,
				&serde_json::json!({ "body": comment.as_ref() }),
			)
			.await
			.map(|_| ())
	}

	pub async fn assign_issue<A, B>(
		&self,
		repo_name: A,
		issue_id: i64,
		assignee_login: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let url = format!(
			"{base_url}/{repo}/issues/{issue_id}/assignees",
			base_url = &self.organization.repos_url,
			repo = repo_name.as_ref(),
			issue_id = issue_id
		);
		self.client
			.post_response(
				&url,
				&serde_json::json!({ "assignees": [assignee_login.as_ref()] }),
			)
			.await
			.map(|_| ())
	}

	pub async fn close_issue<A>(
		&self,
		repo_name: A,
		issue_number: i64,
	) -> Result<()>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues/{issue_number}",
			base_url = Self::BASE_URL,
			owner = &self.organization.login,
			repo = repo_name.as_ref(),
			issue_number = issue_number
		);
		self.client
			.patch_response(&url, &serde_json::json!({ "state": "closed" }))
			.await
			.map(|_| ())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_issues() {
		dotenv::dotenv().ok();

		let github_organization =
			dotenv::var("GITHUB_ORGANIZATION").expect("GITHUB_ORGANIZATION");
		let github_token = dotenv::var("GITHUB_TOKEN").expect("GITHUB_TOKEN");

		let mut rt = tokio::runtime::Runtime::new().expect("runtime");
		rt.block_on(async {
			let github_bot =
				GithubBot::new(&github_organization, &github_token)
					.await
					.expect("github_bot");
			let repo = github_bot
				.repository("parity-processbot")
				.await
				.expect("repository");
			let created_issue = github_bot
				.create_issue(
					"parity-processbot",
					"testing issue",
					"this is a test",
					"sjeohp",
				)
				.await
				.expect("create_issue");
			let issues =
				github_bot.repository_issues(&repo).await.expect("issues");
			assert!(issues.iter().any(|is| is.title == "testing issue"));
			github_bot
				.create_issue_comment(
					"parity-processbot",
					created_issue.number.expect("created issue id"),
					"testing comment",
				)
				.await
				.expect("create_issue_comment");
			let created_pr = github_bot
				.create_pull_request(
					"parity-processbot",
					"testing pr",
					&format!(
						"Fixes #{}",
						created_issue.number.expect("created issue number")
					),
					"testing_branch",
					"other_testing_branch",
				)
				.await
				.expect("create_pull_request");
			let pr_issues = github_bot
				.pull_request_issues(&repo, &created_pr)
				.await
				.expect("issue");
			assert!(pr_issues.iter().any(|x| x.number == created_issue.number));
			github_bot
				.close_issue(
					"parity-processbot",
					created_issue.number.expect("created issue number"),
				)
				.await
				.expect("close_pull_request");
			let issues = github_bot
				.repository_issues(&repo)
				.await
				.expect("pull_requests");
			assert!(!issues.iter().any(|pr| pr.title == "testing issue"));
			github_bot
				.close_pull_request(
					"parity-processbot",
					created_pr.number.expect("created pr number"),
				)
				.await
				.expect("close_pull_request");
		});
	}
}
