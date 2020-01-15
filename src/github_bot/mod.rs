use crate::{error, github, Result};

use snafu::OptionExt;

pub mod issue;
pub mod project;
pub mod pull_request;
pub mod repository;
pub mod review;

pub struct GithubBot {
	client: crate::http::Client,
	organization: github::Organization,
}

impl GithubBot {
	const BASE_URL: &'static str = "https://api.github.com";

	/// Creates a new instance of `GithubBot` from a GitHub organization defined
	/// by `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organization does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub async fn new<A: AsRef<str>, I: Into<String>>(
		org: A,
		auth_key: I,
	) -> Result<Self> {
		let client = crate::http::Client::new(auth_key);

		let organization = client
			.get(&format!("{}/orgs/{}", Self::BASE_URL, org.as_ref()))
			.await?;

		Ok(Self {
			client,
			organization,
		})
	}

	/// Returns statuses associated with a pull request.
	pub async fn statuses(
		&self,
		pull_request: &github::PullRequest,
	) -> Result<Option<Vec<github::Status>>> {
		if let Some(github::StatusesLink { href }) = pull_request
			.links
			.as_ref()
			.and_then(|links| links.statuses_link.as_ref())
		{
			self.client.get(href).await.map(Some)
		} else {
			Ok(None)
		}
	}

	/// Returns the contents of a file in a repository.
	pub async fn contents(
		&self,
		repo_name: &str,
		path: &str,
	) -> Result<github::Contents> {
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo_name}/contents/{path}",
				base_url = Self::BASE_URL,
				owner = self.organization.login,
				repo_name = repo_name,
				path = path
			))
			.await
	}

	/// Returns the team with a given team slug (eg. 'core-devs').
	pub async fn team(&self, slug: &str) -> Result<github::Team> {
		let url = self.organization.url.as_ref().context(error::MissingData)?;

		self.client.get(format!("{}/teams/{}", url, slug)).await
	}

	/// Returns members of the team with a id.
	pub async fn team_members(
		&self,
		team_id: i64,
	) -> Result<Vec<github::User>> {
		self.client
			.get(format!("{}/teams/{}/members", Self::BASE_URL, team_id))
			.await
	}
}
