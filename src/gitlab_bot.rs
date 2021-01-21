use crate::{error::*, Result};
use reqwest::{header, Client};
use serde::Deserialize;
use url::Url;

pub struct GitlabBot {
	urls: UrlBuilder,
	client: Client,
}

#[derive(PartialEq)]
pub enum JobStatus {
	Started,
	AlreadyRunning,
	Finished,
	Unknown,
}

pub struct Job {
	pub status: JobStatus,
	pub status_raw: String,
	pub url: String,
}

#[derive(Deserialize, Debug)]
struct GitlabJob {
	id: i64,
	name: String,
	status: String,
	web_url: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Pipeline {
	id: i64,
}

impl GitlabBot {
	pub async fn new_with_token(
		hostname: &str,
		project: &str,
		private_token: &str,
	) -> Result<Self> {
		let urls = UrlBuilder::new(hostname, project)?;

		let mut headers = header::HeaderMap::new();
		let bearer = format!("Bearer {}", private_token);
		headers.insert(
			header::AUTHORIZATION,
			header::HeaderValue::from_str(&bearer)?,
		);

		let client = Client::builder()
			.default_headers(headers)
			.timeout(std::time::Duration::from_secs(30))
			.build()?;

		// This request is just for checking that Gitlab is available and the token is valid.
		let project_url = urls.project_url()?;
		client.get(project_url).send().await?;

		Ok(Self {
			urls,
			client: client.to_owned(),
		})
	}

	pub async fn create_file(
		&self,
		path: &str,
		branch: &str,
		commit_msg: &str,
		content: &str,
	) -> Result<()> {
		let body = serde_json::json!({
			"author_name": "processbot",
			"branch": branch,
			"commit_message": commit_msg,
			"content": content
		});

		let url = self.urls.create_file_url(path)?;
		self.client.post(url).json(&body).send().await?;
		Ok(())
	}
}

struct UrlBuilder {
	base_url: Url,
	base_path: Vec<String>,
}

impl UrlBuilder {
	pub fn new(hostname: &str, project: &str) -> Result<Self> {
		let base_url_str = format!("https://{}", hostname);

		let base_url = Url::parse(&base_url_str)
			.or_else(|e| Err(Error::ParseUrl { source: e }))?;

		let base_path = vec!["api", "v4", "projects", &project]
			.into_iter()
			.map(|s| s.to_string())
			.collect();

		Ok(Self {
			base_url,
			base_path,
		})
	}

	pub fn project_url(&self) -> Result<Url> {
		let mut project_url = self.base_url.clone();

		{
			let mut path_segments =
				project_url.path_segments_mut().or_else(|()| {
					Err(Error::UrlCannotBeBase {
						url: self.base_url.to_string(),
					})
				})?;
			path_segments.extend(&self.base_path);
		}

		Ok(project_url)
	}

	pub fn create_file_url(&self, path: &str) -> Result<Url> {
		let mut create_file_url = self.base_url.clone();

		{
			let mut path_segments =
				create_file_url.path_segments_mut().or_else(|()| {
					Err(Error::UrlCannotBeBase {
						url: self.base_url.to_string(),
					})
				})?;

			path_segments.extend(&self.base_path);
			path_segments.extend(&["repository", "files", &path.to_string()]);
		}

		Ok(create_file_url)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn builder() -> UrlBuilder {
		UrlBuilder::new("gitlab.parity.io", "parity/processbot-test-repo")
			.unwrap()
	}

	fn assert_url(url: Result<Url>, expected: &str) {
		assert!(url.is_ok());
		assert_eq!(&url.unwrap().to_string(), expected);
	}

	#[test]
	fn test_project_url() {
		let project_url = builder().project_url();

		assert_url(
			project_url,
			"https://gitlab.parity.io/api/v4/projects/parity%2Fprocessbot-test-repo"
		);
	}

	#[test]
	fn test_create_file_url() {
		let cf_url =
			builder().create_file_url("requests/request-1610469388.toml");

		assert_url(
			cf_url,
			"https://gitlab.parity.io/api/v4/projects/parity%2Fprocessbot-test-repo/repository/files/requests%2Frequest-1610469388.toml"
		);
	}
}
