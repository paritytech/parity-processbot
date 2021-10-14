use rocksdb::DB;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
mod logging;

use parity_processbot::{
	config::MainConfig, constants::*, github::Payload, github_bot, server::*,
	webhook::*,
};

fn main() -> anyhow::Result<()> {
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.format(logging::gke::format)
		.init();

	let config = MainConfig::from_env();

	let socket = SocketAddr::new(
		IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
		config.webhook_port.parse::<u16>().expect("webhook port"),
	);

	let db_version_path =
		Path::new(&config.db_path).join("__PROCESSBOT_VERSION__");
	let is_at_current_db_version = match db_version_path.exists() {
		true => {
			let str = fs::read_to_string(&db_version_path)?;
			str == DATABASE_VERSION
		}
		false => false,
	};
	if !is_at_current_db_version {
		log::info!(
			"Clearing database to start from version {}",
			DATABASE_VERSION
		);
		for entry in fs::read_dir(&config.db_path)? {
			let entry = entry?;
			if entry.path() == db_version_path {
				continue;
			}
			if entry.metadata()?.is_dir() {
				fs::remove_dir_all(entry.path())?;
			} else {
				fs::remove_file(entry.path())?;
			}
		}
		fs::write(db_version_path, DATABASE_VERSION)?;
	}

	let db = DB::open_default(&config.db_path)?;

	let github_bot = github_bot::GithubBot::new(
		config.private_key.clone(),
		&config.installation_login,
		config.github_app_id,
	)?;

	let webhook_proxy_url = config.webhook_proxy_url.clone();

	let app_state = Arc::new(Mutex::new(AppState {
		db,
		github_bot,
		config,
	}));

	let rt = tokio::runtime::Builder::new()
		.threaded_scheduler()
		.enable_all()
		.build()?;

	if let Some(webhook_proxy_url) = webhook_proxy_url {
		use eventsource::reqwest::Client;
		use reqwest::Url;

		let webhook_proxy_url = webhook_proxy_url.to_string();
		let client = Client::new(Url::parse(&webhook_proxy_url).unwrap());

		#[derive(serde::Deserialize)]
		struct SmeePayload {
			body: Payload,
		}
		for event in client {
			let state = app_state.clone();
			rt.spawn(async move {
				let event = event.unwrap();

				if let Ok(payload) =
					serde_json::from_str::<SmeePayload>(event.data.as_str())
				{
					let state = &*state.lock().await;
					let (merge_cancel_outcome, result) =
						handle_payload(payload.body, &state).await;
					if let Err(err) = result {
						handle_error(merge_cancel_outcome, err, &state).await;
					}
				} else {
					match event.event_type.as_ref().map(|t| t.as_str()) {
						Some("ping") => (),
						Some("ready") => log::info!("Webhook proxy is ready!"),
						_ => log::info!("Not parsed {:?}", event),
					}
				}
			});
		}
	} else {
		rt.spawn(init_server(socket, app_state));
	}

	loop {}
}
