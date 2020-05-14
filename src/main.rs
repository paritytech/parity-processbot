use actix_web::{App, HttpServer};
use parking_lot::RwLock;
use rocksdb::DB;
use snafu::ResultExt;
use std::{sync::Arc, time::Duration};

use parity_processbot::{
	bamboo,
	config::{BotConfig, MainConfig},
	error, github_bot,
	webhook::*,
};

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
	match run().await {
		Err(error) => panic!("{}", error),
		_ => Ok(()),
	}
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
	let config = MainConfig::from_env();
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.init();

	let db = Arc::new(RwLock::new(DB::open_default(&config.db_path)?));

	/*
	log::info!(
		"Connecting to Matrix homeserver {}",
		config.matrix_homeserver,
	);
	let matrix_bot = matrix_bot::MatrixBot::new_with_token(
		&config.matrix_homeserver,
		&config.matrix_access_token,
		&config.matrix_default_channel_id,
		config.matrix_silent,
	)?;
	*/

	log::info!("Connecting to Github account {}", config.installation_login);
	let github_bot = github_bot::GithubBot::new(
		config.private_key.clone(),
		&config.installation_login,
	)
	.await?;

	// the bamboo queries can take a long time so only wait for it
	// on launch. subsequently update in the background.
	if db.read().get(BAMBOO_DATA_KEY).ok().flatten().is_none() {
		log::info!("Waiting for Bamboo data (may take a few minutes)");
		match bamboo::github_to_matrix(&config.bamboo_token) {
			Ok(h) => db
				.write()
				.put(
					BAMBOO_DATA_KEY,
					bincode::serialize(&h).expect("serialize bamboo"),
				)
				.expect("put bamboo"),
			Err(e) => log::error!("Bamboo error: {}", e),
		}
	}

	let config_clone = config.clone();
	let db_clone = db.clone();

	// update github_to_matrix on another thread
	std::thread::spawn(move || loop {
		log::info!("Updating Bamboo data");
		match bamboo::github_to_matrix(&config_clone.bamboo_token) {
			Ok(h) => db_clone
				.write()
				.put(
					BAMBOO_DATA_KEY,
					bincode::serialize(&h).expect("serialize bamboo"),
				)
				.expect("put bamboo"),
			Err(e) => log::error!("Bamboo error: {}", e),
		}
		std::thread::sleep(Duration::from_secs(config_clone.bamboo_tick_secs));
	});

	log::info!("webhook secret {:?}", &config.webhook_secret);
	let app_state = Arc::new(AppState {
		db: db,
		github_bot: github_bot,
		//		matrix_bot: matrix_bot,
		bot_config: BotConfig::from_env(),
		webhook_secret: config.webhook_secret,
		environment: config.environment,
		test_repo: config.test_repo,
	});

	let addr = format!("0.0.0.0:{}", config.webhook_port);
	log::info!("Listening on {}", addr);
	Ok(HttpServer::new(move || {
		App::new().data(app_state.clone()).service(webhook)
	})
	.bind(&addr)?
	.run()
	.await
	.context(error::Actix)?)
}

#[cfg(test)]
mod tests {
	use regex::Regex;

	#[test]
	fn test_replace_whitespace_in_toml_key() {
		let mut s = String::from("[Smart Contracts Ok]\nwhitelist = []");
		let re = Regex::new(
			r"^\[((?:[[:word:]]|[[:punct:]])*)[[:blank:]]((?:[[:word:]]|[[:punct:]])*)",
		)
		.unwrap();
		while re.captures_iter(&s).count() > 0 {
			s = dbg!(re.replace_all(&s, "[$1-$2").to_string());
		}
		assert_eq!(&s, "[Smart-Contracts-Ok]\nwhitelist = []");
	}
}
