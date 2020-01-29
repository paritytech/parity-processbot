use parking_lot::RwLock;
use rocksdb::DB;
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, sync::Arc, time::Duration};

use parity_processbot::{bamboo, bots, config, error, github_bot, matrix_bot};

const GITHUB_TO_MATRIX_KEY: &str = "GITHUB_TO_MATRIX";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	match run().await {
		Err(error) => panic!("{}", error),
		_ => Ok(()),
	}
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
	let config = config::MainConfig::from_env();
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.init();

	let db = DB::open_default(&config.db_path)?;

	let matrix_bot = matrix_bot::MatrixBot::new(
		&config.matrix_homeserver,
		&config.matrix_user,
		&config.matrix_password,
		&config.matrix_default_channel_id,
	)?;
	log::info!(
		"[+] Connected to matrix homeserver {} as {}",
		config.matrix_homeserver,
		config.matrix_user
	);

	let github_bot =
		github_bot::GithubBot::new(config.private_key.clone()).await?;
	log::info!("[+] Connected to github");

	// the bamboo queries can take a long time so only wait for it
	// if github_to_matrix is not in the db. otherwise update it
	// in the background and start the main loop
	if db
		.get_pinned(&GITHUB_TO_MATRIX_KEY)
		.context(error::Db)?
		.is_none()
	{
		// block on bamboo
		let github_to_matrix =
			dbg!(bamboo::github_to_matrix(&config.bamboo_token))?;
		db.put(
			&GITHUB_TO_MATRIX_KEY,
			serde_json::to_string(&github_to_matrix)
				.context(error::Json)?
				.as_bytes(),
		)
		.context(error::Db)?;
	}

	let db = Arc::new(RwLock::new(db));

	let db_clone = db.clone();
	let config_clone = config.clone();

	// update github_to_matrix on another thread
	std::thread::spawn(move || loop {
		match bamboo::github_to_matrix(&config_clone.bamboo_token).and_then(
			|github_to_matrix| {
				let db = db_clone.write();
				db.delete(&GITHUB_TO_MATRIX_KEY)
					.context(error::Db)
					.map(|_| {
						serde_json::to_string(&github_to_matrix)
							.context(error::Json)
							.map(|s| {
								db.put(&GITHUB_TO_MATRIX_KEY, s.as_bytes())
									.context(error::Db)
							})
					})
			},
		) {
			Ok(_) => {}
			Err(e) => log::error!("DB error: {}", e),
		}
		std::thread::sleep(Duration::from_secs(config_clone.bamboo_tick_secs));
	});

	let mut interval =
		tokio::time::interval(Duration::from_secs(config.main_tick_secs));

	let mut bot =
		bots::Bot::new(db, github_bot, matrix_bot, vec![], HashMap::new());

	loop {
		interval.tick().await;

		let core_devs = bot
			.github_bot
			.team_members(
				bot.github_bot
					.team("core-devs")
					.await?
					.id
					.context(error::MissingData)?,
			)
			.await?;

		let github_to_matrix = bot
			.db
			.read()
			.get(&GITHUB_TO_MATRIX_KEY)
			.context(error::Db)?
			.map(|ref v| {
				serde_json::from_slice::<HashMap<String, String>>(v)
					.context(error::Json)
			})
			.expect(
				"github_to_matrix should always be in the db by this time",
			)?;

		bot.core_devs = core_devs;
		bot.github_to_matrix = github_to_matrix;
		bot.update().await?;
	}
}
