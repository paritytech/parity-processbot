use crate::self::http;

pub struct Bot {
	pub client: http::Client,
	pub base_url: String,
	pub base_html_url: String,
}

impl Bot {
	pub async fn new(
		private_key: impl Into<Vec<u8>>,
		installation_login: String,
		base_url: String,
		base_html_url: String,
	) -> Result<Self> {
		let client = http::Client::new(private_key.into(), installation_login);

		Ok(Self {
			client,
			base_url,
			base_html_url,
		})
	}
}
