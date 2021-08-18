pub struct Config {
	pub installation_login: String,
	pub webhook_secret: String,
	pub webhook_port: String,
	pub db_path: String,
	pub private_key: String,
}

impl Config {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		fn get_env(name: &str) -> String {
			dotenv::var(name).expect("Environment variable was not set: {}")
		}

		let installation_login = get_env("INSTALLATION_LOGIN");
		let webhook_secret = get_env("WEBHOOK_SECRET");
		let webhook_port = get_env("WEBHOOK_PORT");
		let db_path = get_env("DB_PATH");
		let private_key = get_env("PRIVATE_KEY");

		Self {
			installation_login,
			webhook_secret,
			webhook_port,
			db_path,
			private_key,
		}
	}
}
