use rocksdb::DB;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::Mutex;
mod logging;

use parity_processbot::{
	config::MainConfig, github_bot, gitlab_bot, server::*, webhook::*,
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
	match run().await {
		Err(error) => panic!("{}", error),
		_ => Ok(()),
	}
}

async fn run() -> anyhow::Result<()> {
	let config = MainConfig::from_env();
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.format(logging::gke::format)
		.init();

	let db = DB::open_default(&config.db_path)?;

	log::info!("Connecting to Github account {}", config.installation_login);
	let github_bot = github_bot::GithubBot::new(
		config.private_key.clone(),
		&config.installation_login,
	)
	.await?;

	log::info!("Connecting to Gitlab https://{}", config.gitlab_hostname);
	let gitlab_bot = gitlab_bot::GitlabBot::new_with_token(
		&config.gitlab_hostname,
		&config.gitlab_project,
		&config.gitlab_job_name,
		&config.gitlab_private_token,
	)?;

	let app_state = Arc::new(Mutex::new(AppState {
		db: db,
		github_bot: github_bot,
		matrix_bot: matrix_bot,
		gitlab_bot: gitlab_bot,
		webhook_secret: config.webhook_secret,
	}));

	let socket = SocketAddr::new(
		IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
		config.webhook_port.parse::<u16>().expect("webhook port"),
	);

	init_server(socket, app_state).await
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
