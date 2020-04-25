use parking_lot::RwLock;
//use rocksdb::DB;
use futures_util::future::TryFutureExt;
//use snafu::ResultExt;
use std::{collections::HashMap, sync::Arc, time::Duration};

use parity_processbot::{bamboo, bots, config, github_bot, matrix_bot};

//const GITHUB_TO_MATRIX_KEY: &str = "GITHUB_TO_MATRIX";

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

	//	let db = DB::open_default(&config.db_path)?;

	log::info!(
		"Connecting to matrix homeserver {}",
		config.matrix_homeserver,
	);
	let matrix_bot = matrix_bot::MatrixBot::new_with_token(
		&config.matrix_homeserver,
		&config.matrix_access_token,
		&config.matrix_default_channel_id,
		config.matrix_silent,
	)?;

	log::info!("Connecting to Github account {}", config.installation_login);
	let github_bot = github_bot::GithubBot::new(
		config.private_key.clone(),
		&config.installation_login,
	)
	.await?;

	let mut gtm = HashMap::new();

	// the bamboo queries can take a long time so only wait for it
	// on launch. subsequently update in the background.
	log::info!("Waiting for Bamboo data (may take a few minutes)");
	match bamboo::github_to_matrix(&config.bamboo_token) {
		Ok(h) => gtm = h,
		Err(e) => log::error!("Bamboo error: {}", e),
	}

	let gtm = Arc::new(RwLock::new(gtm));

	let gtm_clone = gtm.clone();
	let config_clone = config.clone();

	// update github_to_matrix on another thread
	std::thread::spawn(move || loop {
		match bamboo::github_to_matrix(&config_clone.bamboo_token) {
			Ok(h) => *gtm_clone.write() = h,
			Err(e) => log::error!("Bamboo error: {}", e),
		}
		std::thread::sleep(Duration::from_secs(config_clone.bamboo_tick_secs));
	});

	let mut interval =
		tokio::time::interval(Duration::from_secs(config.main_tick_secs));

	let mut bot =
		bots::Bot::new(github_bot, matrix_bot, vec![], HashMap::new());

	let mut core_devs = match bot.github_bot.team("core-devs").await {
		Ok(team) => bot.github_bot.team_members(team.id).await?,
		_ => vec![],
	};

	loop {
		interval.tick().await;

		log::info!("Updating core-devs");
		match bot
			.github_bot
			.team("core-devs")
			.and_then(|team| bot.github_bot.team_members(team.id))
			.await
		{
			Ok(members) => core_devs = members,
			Err(e) => log::error!("{}", e),
		};

		log::info!("Cloning things");
		bot.core_devs = core_devs.clone();
		bot.github_to_matrix = gtm.read().clone();

		log::info!("Bot update");
		if let Err(e) = bot.update().await {
			log::error!("{:?}", e);
		}

		log::info!("Sleeping for {} seconds", config.main_tick_secs);
	}
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
