use std::borrow::Cow;
use std::collections::HashMap;
use std::time::SystemTime;

use byteorder::{
	BigEndian,
	ByteOrder,
};
use futures::future::Future;
use hyperx::header::TypedHeaders;
use rocksdb::{
	IteratorMode,
	DB,
};
use serde::*;
use snafu::ResultExt;

use crate::{
	error,
	github,
	pull_request::handle_pull_request,
	Result,
};

pub struct GithubBot {
	client: reqwest::Client,
	auth_key: String,
	organization: github::Organization,
}

impl GithubBot {
	const BASE_URL: &'static str = "https://api.github.com";

	/// Creates a new instance of `GithubBot` from a GitHub organization defined by
	/// `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organization does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub fn new<A: AsRef<str>, I: Into<String>>(org: A, auth_key: I) -> Result<Self> {
		let auth_key = auth_key.into();
		let client = reqwest::Client::new();

		let organization = client
			.get(&format!("https://api.github.com/orgs/{}", org.as_ref()))
			.bearer_auth(&auth_key)
			.send()
			.context(error::Http)?
			.json()
			.context(error::Http)?;

		Ok(Self {
			client,
			organization,
			auth_key,
		})
	}

	/// Returns all of the repositories managed by the organization.
	pub fn repositories(&self) -> Result<Vec<github::Repository>> {
		self.get_all(&self.organization.repos_url)
	}

	/// Returns all of the pull requests in a single repository.
	pub fn pull_requests(&self, repo: &github::Repository) -> Result<Vec<github::PullRequest>> {
		self.get_all(repo.pulls_url.replace("{/number}", ""))
	}

	/// Returns all reviews associated with a pull request.
	pub fn reviews(&self, pull_request: &github::PullRequest) -> Result<Vec<github::Review>> {
		self.get_all(format!("{}/reviews", pull_request.html_url))
	}

	/// Returns all reviews associated with a pull request.
	pub fn issue(&self, pull_request: &github::PullRequest) -> Result<Option<github::Issue>> {
		self.get(&pull_request.links.issue_link.href)
	}

	/// Returns all reviews associated with a pull request.
	pub fn statuses(&self, pull_request: &github::PullRequest) -> Result<Vec<github::Status>> {
		self.get(&pull_request.links.statuses_link.href)
	}

	/// Returns the project info associated with a repository.
	pub fn project_info(&self, repository: &github::Repository) -> Result<github::ProjectInfo> {
		unimplemented!();
	}

	/// Returns the team with a given team slug (eg. 'core-devs').
	pub fn team(&self, slug: &str) -> Result<github::Team> {
		self.get(&format!("{}/teams/{}", self.organization.url, slug))
	}

	/// Returns members of the team with a id.
	pub fn team_members(&self, team_id: i64) -> Result<Vec<github::User>> {
		self.get(&format!("{}/teams/{}/members", Self::BASE_URL, team_id))
	}

	/// Creates a comment in the repo
	pub fn add_comment<A, B>(&self, repo_name: A, issue_id: i64, comment: B) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		log::info!("Adding comment");
		let repo = repo_name.as_ref();
		let comment = comment.as_ref();
		let url = format!(
			"{base}/repos/{org}/{repo}/issues/{issue_id}/comments",
			base = Self::BASE_URL,
			org = self.organization.login,
			repo = repo,
			issue_id = issue_id
		);
		log::info!("POST {}", url);

		self.client
			.post(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({ "body": comment }))
			.send()
			.context(error::Http)
			.and_then(error::map_response_status)
			.map(|_| ())
	}

	pub fn assign_author<A, B>(&self, repo_name: A, issue_id: i64, author_login: B) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let author = author_login.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/{repo}/issues/{issue_id}/assignees",
			base = base,
			repo = repo,
			issue_id = issue_id
		);

		self.client
			.post(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({ "assignees": [author] }))
			.send()
			.context(error::Http)
			.map(|_| ())
	}

	pub fn merge_pull_request<A, B>(&self, repo_name: A, pull_number: i64) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/pulls/{pull_number}/merge",
			base = base,
			owner = self.organization.login,
			repo = repo,
			pull_number = pull_number
		);
		self.client
			.put(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({}))
			.send()
			.context(error::Http)
			.map(|_| ())
	}

	pub fn close_pull_request<A, B>(&self, repo_name: A, pull_number: i64) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/pulls/{pull_number}",
			base = base,
			owner = self.organization.login,
			repo = repo,
			pull_number = pull_number
		);
		self.client
			.patch(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({ "state": "closed" }))
			.send()
			.context(error::Http)
			.map(|_| ())
	}

	pub fn close_issue<A, B>(&self, repo_name: A, issue_id: i64) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let repo = repo_name.as_ref();
		let base = &self.organization.repos_url;
		let url = format!(
			"{base}/repos/{owner}/{repo}/issues/{issue_id}",
			base = base,
			owner = self.organization.login,
			repo = repo,
			issue_id = issue_id
		);
		self.client
			.patch(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({ "state": "closed" }))
			.send()
			.context(error::Http)
			.map(|_| ())
	}

	/// Get a single entry from a resource in GitHub.
	fn get<'b, I, T>(&self, url: I) -> Result<T>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
	{
		let mut response = self
			.client
			.get(&*(url.into()))
			.bearer_auth(&self.auth_key)
			.send()
			.context(error::Http)?;

		response.json::<T>().context(error::Http)
	}

	// Originally adapted from:
	// https://github.com/XAMPPRocky/gh-auditor/blob/ca67641c0a29d64fc5c6b4244b45ae601604f3c1/src/lib.rs#L232-L267
	/// Gets a all entries across all pages from a resource in GitHub.
	fn get_all<'b, I, T>(&self, url: I) -> Result<Vec<T>>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
	{
		let mut entities = Vec::new();
		let mut next = Some(url.into());

		while let Some(url) = next {
			let mut response = self
				.client
				.get(&*url)
				.bearer_auth(&self.auth_key)
				.send()
				.context(error::Http)?;

			next = response
				.headers()
				.decode::<hyperx::header::Link>()
				.ok()
				.and_then(|v| {
					v.values()
						.iter()
						.find(|link| {
							link.rel()
								.map(|rel| rel.contains(&hyperx::header::RelationType::Next))
								.unwrap_or(false)
						})
						.map(|l| l.link())
						.map(str::to_owned)
						.map(Cow::Owned)
				});

			let mut body = response.json::<Vec<T>>().context(error::Http)?;
			entities.append(&mut body);
		}

		Ok(entities)
	}
}
