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
	let tick_secs = dotenv::var("TICK_SECS")
		.expect("TICK_SECS")
		.parse::<u64>()
		.expect("parse tick_secs");

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

	let github_to_matrix = dbg!(bamboo::github_to_matrix(&bamboo_token))?;

	let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));
	loop {
		interval.tick().await;
		bots::update(
			&db,
			&github_bot,
			&matrix_bot,
			&core_devs,
			&github_to_matrix,
		)
		.await?;
	}
}
