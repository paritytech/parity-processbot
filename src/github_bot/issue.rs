use crate::{error, github, Result};

use snafu::{OptionExt, ResultExt};

use super::GithubBot;

use regex::Regex;

impl GithubBot {
	/// Returns a single issue.
	pub async fn issue(
		&self,
		owner: &str,
		repo: &github::Repository,
		number: i64,
	) -> Result<github::Issue> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/issues/{number}",
				base_url = Self::BASE_URL,
				owner = owner,
				repo = repo.name,
				number = number
			))
			.await
	}

	/// Returns all of the issues in a single repository.
	pub async fn repository_issues(
		&self,
		repo: &github::Repository,
	) -> Result<Vec<github::Issue>> {
		let url = repo.issues_url.as_ref().context(error::MissingData)?;
		self.client.get_all(url.replace("{/number}", "")).await
	}

	/// Returns a list of issues mentioned in the body of a pull request.
	pub async fn linked_issues(
		&self,
		owner: &str,
		repo_name: &str,
		body: &str,
	) -> Result<Vec<github::Issue>> {
		let re = Regex::new(r"#([0-9]+)").unwrap();
		Ok(futures::future::join_all(
			re.captures_iter(body)
				.filter_map(|cap| {
					cap.get(1).and_then(|x| x.as_str().parse::<i64>().ok())
				})
				.map(|num| {
					self.client.get(format!(
						"{base_url}/repos/{owner}/{repo}/issues/{issue_number}",
						base_url = Self::BASE_URL,
						owner = owner,
						repo = &repo_name,
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
		owner: &str,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::IssueEvent>> {
		self.client
			.get_all(format!(
                "{base_url}/repos/{owner}/{repo_name}/issues/{issue_number}/events",
                base_url = Self::BASE_URL,
                owner = owner,
                repo_name = repo_name,
                issue_number = issue_number
            ))
			.await
	}

	pub async fn issue_projects<'a>(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
		projects: &'a [github::Project],
	) -> Result<Vec<&'a github::Project>> {
		self.active_project_events(owner, repo_name, issue_number)
			.await
			.map(|v| {
				v.iter()
					.filter_map(|issue_event| {
						projects.iter().find(|proj| {
							issue_event
								.project_card
								.as_ref()
								.expect("issue event project card")
								.id == proj.id
						})
					})
					.collect::<Vec<&'a github::Project>>()
			})
	}

	pub async fn create_issue(
		&self,
		owner: &str,
		repo_name: &str,
		title: &str,
		body: &str,
		assignee: &str,
	) -> Result<github::Issue> {
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
		);
		let params = serde_json::json!({
						"title": title,
						"body": body,
						"assignee": assignee,
		});
		self.client
			.post_response(&url, &params)
			.await?
			.json()
			.await
			.context(error::Http)
	}

	/// Fetches the list of comments on an issue.
	pub async fn get_issue_comments(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Vec<github::Comment>> {
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_number}/comments",
			base = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			issue_number = issue_number
		);
		self.client.get_all(&url).await
	}

	/// Adds a comment to an issue.
	pub async fn create_issue_comment(
		&self,
		owner: &str,
		repo_name: &str,
		issue_number: i64,
		comment: &str,
	) -> Result<()> {
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_number}/comments",
			base = Self::BASE_URL,
			owner = owner,
			repo = repo_name,
			issue_number = issue_number
		);
		self.client
			.post_response(&url, &serde_json::json!({ "body": comment }))
			.await
			.map(|_| ())
	}

	pub async fn assign_issue<A, B>(
		&self,
		owner: &str,
		repo_name: A,
		issue_number: i64,
		assignee_login: B,
	) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues/{issue_number}/assignees",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name.as_ref(),
			issue_number = issue_number
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
		owner: &str,
		repo_name: A,
		issue_number: i64,
	) -> Result<github::Issue>
	where
		A: AsRef<str>,
	{
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/issues/{issue_number}",
			base_url = Self::BASE_URL,
			owner = owner,
			repo = repo_name.as_ref(),
			issue_number = issue_number
		);
		self.client
			.patch_response(&url, &serde_json::json!({ "state": "closed" }))
			.await?
			.json()
			.await
			.context(error::Http)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[ignore]
	#[test]
	fn test_issues() {
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
			let repo = github_bot
				.repository(&test_repo_name)
				.await
				.expect("repository");
			let created_issue = github_bot
				.create_issue(
					&test_repo_name,
					&"testing issue".to_owned(),
					&"this is a test".to_owned(),
					&"sjeohp".to_owned(),
				)
				.await
				.expect("create_issue");
			let issues =
				github_bot.repository_issues(&repo).await.expect("issues");
			assert!(issues.iter().any(|is| is
				.title
				.as_ref()
				.map_or(false, |t| t == "testing issue")));
			github_bot
				.create_issue_comment(
					&test_repo_name,
					created_issue.number,
					&"testing comment".to_owned(),
				)
				.await
				.expect("create_issue_comment");
			let created_pr = github_bot
				.create_pull_request(
					&test_repo_name,
					&"testing pr".to_owned(),
					&format!("Fixes #{}", created_issue.number),
					&"testing_branch".to_owned(),
					&"other_testing_branch".to_owned(),
				)
				.await
				.expect("create_pull_request");
			let pr_issues = github_bot
				.linked_issues(
					&test_repo_name,
					&created_pr.body.expect("created_pr body"),
				)
				.await
				.expect("issue");
			assert!(pr_issues.iter().any(|x| x.number == created_issue.number));
			github_bot
				.close_issue(&test_repo_name, created_issue.number)
				.await
				.expect("close_pull_request");
			let issues = github_bot
				.repository_issues(&repo)
				.await
				.expect("repo issues");
			assert!(!issues.iter().any(|pr| pr
				.title
				.as_ref()
				.map_or(false, |t| t == "testing issue")));
			github_bot
				.close_pull_request(&test_repo_name, created_pr.number)
				.await
				.expect("close_pull_request");
		});
	}
}
