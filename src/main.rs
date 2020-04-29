use actix_web::{post, web, Responder, HttpServer, HttpResponse, App};
use parking_lot::RwLock;
//use rocksdb::DB;
use futures_util::future::TryFutureExt;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt};
use std::{collections::HashMap, sync::Arc, time::Duration};
use std::rc::Rc;
use std::sync::Mutex;

use parity_processbot::{
	bamboo, bots, config, constants::*, error, github::*, github_bot, matrix_bot,
};

//const GITHUB_TO_MATRIX_KEY: &str = "GITHUB_TO_MATRIX";

#[post("/payload")]
async fn webhook(state: web::Data<Arc<github_bot::GithubBot>>, payload: web::Json<Payload>) -> impl Responder {
    log::info!("received");
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
            dbg!(&pr);
            dbg!(&repo_url);
            dbg!(&number);
            let github_bot = state.get_ref();
            if let Some(repo_name) =
                repo_url.rsplit('/').next().map(|s| s.to_string())
            {
                dbg!(&repo_name);
                if let Ok(pr) =
                    github_bot.pull_request(&repo_name, number).await
                {
                    dbg!(&pr);
                    if comment.body.to_lowercase().trim()
                        == AUTO_MERGE_REQUEST.to_lowercase().trim()
                    {
                        log::info!("merge requested");
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
					}
				}
			}
		}
		event => {
            log::info!("Received payload {:?}", event);
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
	let config = config::MainConfig::from_env();
	env_logger::from_env(env_logger::Env::default().default_filter_or("info"))
		.init();

	//	let db = DB::open_default(&config.db_path)?;

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

	let mut gtm = HashMap::new();

	// the bamboo queries can take a long time so only wait for it
	// on launch. subsequently update in the background.
	//	log::info!("Waiting for Bamboo data (may take a few minutes)");
	//	match bamboo::github_to_matrix(&config.bamboo_token) {
	//		Ok(h) => gtm = h,
	//		Err(e) => log::error!("Bamboo error: {}", e),
	//	}

	let gtm = Arc::new(RwLock::new(gtm));

	let gtm_clone = gtm.clone();
	let config_clone = config.clone();

	// update github_to_matrix on another thread
	std::thread::spawn(move || loop {
		log::info!("Updating Bamboo data");
		match bamboo::github_to_matrix(&config_clone.bamboo_token) {
			Ok(h) => *gtm_clone.write() = h,
			Err(e) => log::error!("Bamboo error: {}", e),
		}
		std::thread::sleep(Duration::from_secs(config_clone.bamboo_tick_secs));
	});

	let mut interval =
		tokio::time::interval(Duration::from_secs(config.main_tick_secs));

    let gbot = Arc::new(github_bot);
	Ok(HttpServer::new(move || App::new().data(gbot.clone())
                       .service(webhook))
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
