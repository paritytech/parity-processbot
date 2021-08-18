use crate::self::http;

mod commit;
mod issue;
mod organization;
mod pull_request;
mod repository;
mod review;
mod team;

pub struct GithubBot {
	pub client: http::Client,
}

impl GithubBot {
	pub async fn new(
		private_key: impl Into<Vec<u8>>,
		installation_login: &str,
		base_url: &str,
		base_html_url: &str,
	) -> Result<Self> {
		let client = http::Client::new(
			private_key.into(),
			installation_login.to_owned(),
		);

		Ok(Self {
			client,
			base_url,
			base_html_url,
		})
	}
}
