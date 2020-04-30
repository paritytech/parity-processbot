use actix_web::{post, web, App, HttpResponse, HttpServer, Responder};
use futures_util::future::TryFutureExt;
use parking_lot::RwLock;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use parity_processbot::{
	bamboo, bots,
	config::{BotConfig, MainConfig},
	constants::*,
	error,
	github::*,
	github_bot, matrix_bot,
};

const BAMBOO_DATA_KEY: &str = "BAMBOO_DATA";
const CORE_DEVS_KEY: &str = "CORE_DEVS";

pub struct AppState {
	pub db: Arc<RwLock<DB>>,
	pub github_bot: github_bot::GithubBot,
	pub matrix_bot: matrix_bot::MatrixBot,
	pub config: BotConfig,
}

#[derive(Serialize, Deserialize)]
pub struct MergeRequest {
	sha: String,
}

#[post("/payload")]
async fn webhook(
	state: web::Data<Arc<AppState>>,
	payload: web::Json<Payload>,
) -> impl Responder {
	let db = &state.get_ref().db;
	let github_bot = &state.get_ref().github_bot;
	match payload.into_inner() {
		Payload::IssueComment {
			action: IssueCommentAction::Created,
			issue:
				Issue {
					number,
					pull_request: Some(pr),
					repository_url: Some(repo_url),
					..
				},
			comment,
		} => {
			log::info!("Received issue comment {:?}", comment);
			if let Some(repo_name) =
				repo_url.rsplit('/').next().map(|s| s.to_string())
			{
				if let Ok(PullRequest {
					head:
						Head {
							ref_field,
							sha: head_sha,
							..
						},
					..
				}) = github_bot.pull_request(&repo_name, number).await
				{
					if comment.body.to_lowercase().trim()
						== AUTO_MERGE_REQUEST.to_lowercase().trim()
					{
						log::info!("merge requested");
						db.write().put(
							ref_field.as_bytes(),
							bincode::serialize(&MergeRequest { sha: head_sha })
								.expect("bincode serialize"),
						)
						.expect("db write");

						/*
						let s = String::new();
						if let Ok((reviews, issues, status)) = futures::try_join!(
							github_bot.reviews(&pr),
							github_bot.linked_issues(
								&repo_name,
								pr.body.as_ref().unwrap_or(&s)
							),
							github_bot.status(&repo_name, &pr),
						) {
							let issue_numbers = std::iter::once(pr.number)
								.chain(issues.iter().map(|issue| issue.number))
								.collect::<Vec<i64>>();
						}
						*/
					}
				}
			}
		}
		Payload::CommitStatus {
			state, branches, ..
		} => {
			log::info!("Received commit status {:?}", state);
			for Branch {
				name,
				commit: BranchCommit { sha, .. },
				..
			} in &branches
			{
				match db.read().get(name.as_bytes()) {
					Ok(Some(b)) => {
						let MergeRequest { sha: head_sha } =
							bincode::deserialize(&b)
								.expect("bincode deserialize");
						log::info!(
							"commit branch matches request {} {} {}",
							name,
							sha,
							head_sha
						)
					}
					_ => {}
				}
			}
		}
		event => {
			log::info!("Received unknown event {:?}", event);
		}
	}
	HttpResponse::Ok()
}

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

	log::info!("Connecting to Github account {}", config.installation_login);
	let github_bot = github_bot::GithubBot::new(
		config.private_key.clone(),
		&config.installation_login,
	)
	.await?;

	//	let mut bot =
	//		bots::Bot::new(github_bot, matrix_bot, vec![], HashMap::new());

	let mut core_devs = match github_bot.team("core-devs").await {
		Ok(team) => github_bot.team_members(team.id).await?,
		_ => vec![],
	};

	db.write()
		.put(
			&CORE_DEVS_KEY.as_bytes(),
			bincode::serialize(&core_devs).expect("serialize core-devs"),
		)
		.expect("put core-devs");

	// the bamboo queries can take a long time so only wait for it
	// on launch. subsequently update in the background.
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

	let mut interval =
		tokio::time::interval(Duration::from_secs(config.main_tick_secs));

	let app_state = Arc::new(AppState {
		db: db,
		github_bot: github_bot,
		matrix_bot: matrix_bot,
		config: BotConfig::from_env(),
	});

	Ok(HttpServer::new(move || {
		App::new().data(app_state.clone()).service(webhook)
	})
	.bind("127.0.0.1:4567")?
	.run()
	.await
	.context(error::Actix)?)

	/*
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
	*/
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
