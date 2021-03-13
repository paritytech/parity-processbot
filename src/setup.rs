use crate::{
	config::{BotConfig, MainConfig},
	github_bot, gitlab_bot, logging, matrix_bot,
	webhook::*,
};
use rocksdb::DB;

pub async fn setup(
	conf: Option<MainConfig>,
	bot_config: Option<BotConfig>,
	matrix_bot: Option<matrix_bot::MatrixBot>,
	gitlab_bot: Option<gitlab_bot::GitlabBot>,
	github_bot: Option<github_bot::GithubBot>,
	should_init_logger: bool,
) -> anyhow::Result<AppState> {
	let config = conf.unwrap_or_else(|| MainConfig::from_env());
	let bot_config = bot_config.unwrap_or_else(|| BotConfig::from_env());

	if should_init_logger {
		env_logger::from_env(
			env_logger::Env::default().default_filter_or("info"),
		)
		.format(logging::gke::format)
		.init();
	}

	let db = DB::open_default(&config.db_path)?;

	let matrix_bot = if let Some(matrix_bot) = matrix_bot {
		matrix_bot
	} else {
		log::info!(
			"Connecting to Matrix homeserver {}",
			config.matrix_homeserver,
		);
		matrix_bot::MatrixBot::new_with_token(
			&config.matrix_homeserver,
			&config.matrix_access_token,
			&config.matrix_default_channel_id,
			config.matrix_silent,
		)?
	};

	let github_bot = if let Some(github_bot) = github_bot {
		github_bot
	} else {
		log::info!(
			"Connecting to Github account {}",
			config.installation_login
		);
		github_bot::GithubBot::new(
			config.private_key.clone(),
			&config.installation_login,
			config.github_app_id,
		)
		.await?
	};

	let gitlab_bot = if let Some(gitlab_bot) = gitlab_bot {
		gitlab_bot
	} else {
		log::info!("Connecting to Gitlab https://{}", config.gitlab_hostname);
		gitlab_bot::GitlabBot::new_with_token(
			&config.gitlab_hostname,
			&config.gitlab_project,
			&config.gitlab_job_name,
			&config.gitlab_private_token,
		)?
	};

	// the bamboo queries can take a long time so only wait for it
	// on launch. subsequently update in the background.
	/*
	{
		let db_write = db.write();
		if db_write.get(BAMBOO_DATA_KEY).ok().flatten().is_none() {
			log::info!("Waiting for Bamboo data (may take a few minutes)");
			match bamboo::github_to_matrix(&config.bamboo_token) {
				Ok(h) => db_write
					.put(
						BAMBOO_DATA_KEY,
						bincode::serialize(&h).expect("serialize bamboo"),
					)
					.expect("put bamboo"),
				Err(e) => log::error!("Bamboo error: {}", e),
			}
		}
	}
	*/

	// let config_clone = config.clone();
	//	let db_clone = db.clone();
	/*
	std::thread::spawn(move || loop {
		{
			let db_write = db_clone.write();
			match bamboo::github_to_matrix(&config_clone.bamboo_token) {
				Ok(h) => {
					db_write
						.put(
							BAMBOO_DATA_KEY,
							bincode::serialize(&h).expect("serialize bamboo"),
						)
						.expect("put bamboo");
				},
				Err(e) => log::error!("Bamboo error: {}", e),
			}
		}
		std::thread::sleep(Duration::from_secs(config_clone.bamboo_tick_secs));
	});
	*/

	Ok(AppState {
		db,
		github_bot,
		matrix_bot,
		gitlab_bot,
		bot_config,
		config,
	})
}
