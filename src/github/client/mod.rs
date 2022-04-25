use crate::config::MainConfig;

mod commit;
mod file;
mod issue;
mod org;
mod pull_request;

pub struct GithubClient {
	pub client: crate::http::Client,
	github_api_url: String,
}

impl GithubClient {
	pub fn new(config: &MainConfig) -> Self {
		let client = crate::http::Client::new(config);

		Self {
			client,
			github_api_url: config.github_api_url.clone(),
		}
	}
}
