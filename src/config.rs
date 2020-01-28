#[derive(Debug, Clone)]
pub struct Config {
	pub db_path: String,
	pub bamboo_token: String,
	pub private_key: Vec<u8>,
	pub matrix_homeserver: String,
	pub matrix_user: String,
	pub matrix_password: String,
	pub matrix_default_channel_id: String,
	pub tick_secs: u64,
	pub bamboo_tick_secs: u64,
}

impl Config {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		let db_path = dotenv::var("DB_PATH").expect("DB_PATH");
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
		let matrix_homeserver =
			dotenv::var("MATRIX_HOMESERVER").expect("MATRIX_HOMESERVER");
		let matrix_user = dotenv::var("MATRIX_USER").expect("MATRIX_USER");
		let matrix_password =
			dotenv::var("MATRIX_PASSWORD").expect("MATRIX_PASSWORD");
		let matrix_default_channel_id =
			dotenv::var("MATRIX_DEFAULT_CHANNEL_ID")
				.expect("MATRIX_DEFAULT_CHANNEL_ID");
		let tick_secs = dotenv::var("TICK_SECS")
			.expect("TICK_SECS")
			.parse::<u64>()
			.expect("parse tick_secs");
		let bamboo_tick_secs = dotenv::var("BAMBOO_TICK_SECS")
			.expect("BAMBOO_TICK_SECS")
			.parse::<u64>()
			.expect("parse bamboo_tick_secs");

		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		Self {
			db_path,
			bamboo_token,
			private_key,
			matrix_homeserver,
			matrix_user,
			matrix_password,
			matrix_default_channel_id,
			tick_secs,
			bamboo_tick_secs,
		}
	}
}
