#[derive(Debug, Clone)]
pub struct MainConfig {
	pub installation_login: String,
	pub webhook_secret: String,
	pub webhook_port: String,
	pub db_path: String,
	pub private_key: Vec<u8>,
	pub webhook_proxy_url: Option<String>,
	pub github_app_id: usize
}

impl MainConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		let installation_login =
			dotenv::var("INSTALLATION_LOGIN").expect("INSTALLATION_LOGIN");
		let webhook_secret =
			dotenv::var("WEBHOOK_SECRET").expect("WEBHOOK_SECRET");
		let webhook_port = dotenv::var("WEBHOOK_PORT").expect("WEBHOOK_PORT");
		let db_path = dotenv::var("DB_PATH").expect("DB_PATH");

		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		let webhook_proxy_url = dotenv::var("WEBHOOK_PROXY_URL").ok();
		let github_app_id = dotenv::var("GITHUB_APP_ID")
			.expect("GITHUB_APP_ID")
			.parse::<usize>()
			.expect("GITHUB_APP_ID should be a number");

		Self {
			installation_login,
			webhook_secret,
			webhook_port,
			db_path,
			private_key,
			webhook_proxy_url,
			github_app_id,
		}
	}
}
