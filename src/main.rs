use rocksdb::DB;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::Mutex;
mod logging;

use parity_processbot::{
	config::Config, github_bot, gitlab_bot, server::*, webhook::*,
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
	match run().await {
		Err(error) => panic!("{}", error),
		_ => Ok(()),
	}
}

async fn run() -> anyhow::Result<()> {
	let config = Config::from_env();
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

	let app_state = Arc::new(Mutex::new(AppState {
		db: db,
		github_bot: github_bot,
		webhook_secret: config.webhook_secret,
	}));

	let socket = SocketAddr::new(
		IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
		config.webhook_port.parse::<u16>().expect("webhook port"),
	);

	init_server(socket, app_state).await
}
