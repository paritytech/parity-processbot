use rocksdb::DB;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
mod logging;
use parity_processbot::{
	error::Bincode, webhook::checks_and_status, MergeCancelOutcome,
};
use snafu::ResultExt;
use std::{thread, time::Duration};

use parity_processbot::{
	config::MainConfig, constants::*, github::Payload, github_bot, server,
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

	let github_bot = github_bot::GithubBot::new(&config);

	let webhook_proxy_url = config.webhook_proxy_url.clone();

	let app_state = Arc::new(Mutex::new(AppState {
		db,
		github_bot,
		config,
	}));

	// Poll for pending merge requests
	{
		const DELAY: Duration = Duration::from_secs(30 * 60);
		let state = app_state.clone();
		let mut rt = tokio::runtime::Builder::new()
			.threaded_scheduler()
			.enable_all()
			.build()?;
		thread::spawn(move || loop {
			log::info!("Acquiring poll lock");

			rt.block_on(async {
				let state = &*state.lock().await;

				/*
					Set up a loop for reinitializing the DB's iterator since the operations
					performed in this loop might modify or delete multiple items from the
					database, thus potentially making the iteration not work according to
					expectations.
				*/
				let mut processed_mrs = vec![];
				'db_iteration_loop: loop {
					let db_iter =
						state.db.iterator(rocksdb::IteratorMode::Start);
					for (key, value) in db_iter {
						match bincode::deserialize::<MergeRequest>(&value)
							.context(Bincode)
						{
							Ok(mr) => {
								if processed_mrs.iter().any(
									|prev_mr: &MergeRequest| {
										mr.owner == prev_mr.owner
											&& mr.repo == prev_mr.repo && mr.number
											== prev_mr.number
									},
								) {
									continue;
								}

								// It's only worthwhile to try merging this MR if it has no pending
								// dependencies
								if mr
									.dependencies
									.as_ref()
									.map(|vec| vec.is_empty())
									.unwrap_or(true)
								{
									log::info!(
										"Attempting to resume merge request processing during poll: {:?}",
										mr
									);

									if let Err(err) =
										checks_and_status(state, &mr.sha).await
									{
										let _ = cleanup_pr(
											state,
											&mr.sha,
											&mr.owner,
											&mr.repo,
											mr.number,
											&PullRequestCleanupReason::Error,
										)
										.await;
										handle_error(
											MergeCancelOutcome::WasCancelled,
											err,
											state,
										)
										.await;
									}

									processed_mrs.push(mr);
									continue 'db_iteration_loop;
								}
							}
							Err(err) => {
								log::error!(
									"Failed to deserialize key {} from the database due to {:?}",
									String::from_utf8_lossy(&key),
									err
								);
								let _ = state.db.delete(&key);
							}
						}
					}
					break;
				}
			});

			log::info!("Releasing poll lock");
			thread::sleep(DELAY);
		});
	}

	let mut rt = tokio::runtime::Builder::new()
		.threaded_scheduler()
		.enable_all()
		.build()?;

	if let Some(webhook_proxy_url) = webhook_proxy_url {
		use eventsource::reqwest::Client;
		use reqwest::Url;

		let client = Client::new(Url::parse(&webhook_proxy_url).unwrap());

		#[derive(serde::Deserialize)]
		struct SmeePayload {
			body: Payload,
		}
		for event in client {
			let state = app_state.clone();
			rt.block_on(async move {
				let event = event.unwrap();

				if let Ok(payload) =
					serde_json::from_str::<SmeePayload>(event.data.as_str())
				{
					log::info!("Acquiring lock");
					let state = &*state.lock().await;
					let (merge_cancel_outcome, result) =
						handle_payload(payload.body, state).await;
					if let Err(err) = result {
						handle_error(merge_cancel_outcome, err, state).await;
					}
					log::info!("Releasing lock");
				} else {
					match event.event_type.as_deref() {
						Some("ping") => (),
						Some("ready") => log::info!("Webhook proxy is ready!"),
						_ => log::info!("Not parsed: {:?}", event),
					}
				}
			});
		}
	} else {
		rt.block_on(server::init(socket, app_state))?;
	}

	Ok(())
}
