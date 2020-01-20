use rocksdb::DB;
use snafu::OptionExt;
use std::time::Duration;

use parity_processbot::{bamboo, bots, error, github_bot, matrix_bot};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	match run().await {
		Err(error) => panic!("{}", error),
		_ => Ok(()),
	}
}

struct Config {
	db_path: String,
	bamboo_token: String,
	private_key: Vec<u8>,
	matrix_homeserver: String,
	matrix_user: String,
	matrix_password: String,
	matrix_default_channel_id: String,
	tick_secs: u64,
}

impl Config {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		let db_path = dotenv::var("DB_PATH").expect("DB_PATH");
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
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
		}
	}
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
	let config = Config::from_env();
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.init();

	let db = DB::open_default(&config.db_path)?;

	let matrix_bot = matrix_bot::MatrixBot::new(
		&config.matrix_homeserver,
		&config.matrix_user,
		&config.matrix_password,
	)?;
	log::info!(
		"[+] Connected to matrix homeserver {} as {}",
		config.matrix_homeserver,
		config.matrix_user
	);

	let github_bot = github_bot::GithubBot::new(config.private_key).await?;
	log::info!("[+] Connected to github");

	let core_devs = github_bot
		.team_members(
			github_bot
				.team("core-devs")
				.await?
				.id
				.context(error::MissingData)?,
		)
		.await?;

	let github_to_matrix = bamboo::github_to_matrix(&config.bamboo_token)?;

	let mut interval =
		tokio::time::interval(Duration::from_secs(config.tick_secs));
	loop {
		interval.tick().await;
		bots::update(
			&db,
			&github_bot,
			&matrix_bot,
			&core_devs,
			&github_to_matrix,
			&config.matrix_default_channel_id,
		)
		.await?;
	}
}
