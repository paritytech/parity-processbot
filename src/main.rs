use parking_lot::RwLock;
use rocksdb::DB;
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, sync::Arc, time::Duration};

use parity_processbot::{
	bamboo, bots, constants, error, github_bot, matrix_bot,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	match run().await {
		Err(error) => panic!("{}", error),
		_ => Ok(()),
	}
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.init();

	dotenv::dotenv().ok();
	let db_path = dotenv::var("DB_PATH").expect("DB_PATH");
	let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
	let github_organization =
		dotenv::var("GITHUB_ORGANIZATION").expect("GITHUB_ORGANIZATION");
	let github_token = dotenv::var("GITHUB_TOKEN").expect("GITHUB_TOKEN");
	let matrix_homeserver =
		dotenv::var("MATRIX_HOMESERVER").expect("MATRIX_HOMESERVER");
	let matrix_user = dotenv::var("MATRIX_USER").expect("MATRIX_USER");
	let matrix_password =
		dotenv::var("MATRIX_PASSWORD").expect("MATRIX_PASSWORD");
	let matrix_default_channel_id = dotenv::var("MATRIX_DEFAULT_CHANNEL_ID")
		.expect("MATRIX_DEFAULT_CHANNEL_ID");
	let tick_secs = dotenv::var("TICK_SECS")
		.expect("TICK_SECS")
		.parse::<u64>()
		.expect("parse tick_secs");
	let bamboo_tick_secs = dotenv::var("BAMBOO_TICK_SECS")
		.expect("BAMBOO_TICK_SECS")
		.parse::<u64>()
		.expect("parse bamboo tick secs");

	let db = DB::open_default(db_path)?;

	let matrix_bot = matrix_bot::MatrixBot::new(
		&matrix_homeserver,
		&matrix_user,
		&matrix_password,
	)?;
	log::info!(
		"[+] Connected to matrix homeserver {} as {}",
		matrix_homeserver,
		matrix_user
	);

	let github_bot =
		github_bot::GithubBot::new(&github_organization, &github_token).await?;
	log::info!(
		"[+] Connected to github organisation {}",
		github_organization
	);

	let core_devs = github_bot
		.team_members(
			github_bot
				.team("core-devs")
				.await?
				.id
				.context(error::MissingData)?,
		)
		.await?;
	if db
		.get_pinned(&constants::GITHUB_TO_MATRIX_KEY)
		.context(error::Db)?
		.is_none()
	{
		// block on bamboo
		let github_to_matrix = dbg!(bamboo::github_to_matrix(&bamboo_token))?;
		db.put(
			&constants::GITHUB_TO_MATRIX_KEY,
			serde_json::to_string(&github_to_matrix)
				.context(error::Json)?
				.as_bytes(),
		)
		.context(error::Db)?;
	}

	let db = Arc::new(RwLock::new(db));
	let db_bamboo = db.clone();

	std::thread::spawn(move || loop {
		match bamboo::github_to_matrix(&bamboo_token).and_then(
			|github_to_matrix| {
				let db = db_bamboo.write();
				db.delete(&constants::GITHUB_TO_MATRIX_KEY)
					.context(error::Db)
					.map(|_| {
						serde_json::to_string(&github_to_matrix)
							.context(error::Json)
							.map(|s| {
								db.put(
									&constants::GITHUB_TO_MATRIX_KEY,
									s.as_bytes(),
								)
								.context(error::Db)
							})
					})
			},
		) {
			Ok(_) => {}
			Err(e) => log::error!("DB error: {}", e),
		}
		std::thread::sleep(Duration::from_secs(bamboo_tick_secs));
	});

	let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));
	loop {
		interval.tick().await;

		let github_to_matrix = db
			.read()
			.get(&constants::GITHUB_TO_MATRIX_KEY)
			.context(error::Db)?
			.map(|ref v| {
				serde_json::from_slice::<HashMap<String, String>>(v)
					.context(error::Json)
			})
			.expect("broken db")?;

		bots::update(
			&db,
			&github_bot,
			&matrix_bot,
			&core_devs,
			&github_to_matrix,
			&matrix_default_channel_id,
		)
		.await?;
	}
}
