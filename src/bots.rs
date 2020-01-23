use parking_lot::RwLock;
use rocksdb::DB;
use snafu::ResultExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
	error, github, github_bot::GithubBot, issue::handle_issue,
	matrix_bot::MatrixBot, process, pull_request::handle_pull_request, Result,
};

fn process_from_contents(
	c: github::Contents,
) -> Result<impl Iterator<Item = (String, process::ProcessInfo)>> {
	base64::decode(&c.content.replace("\n", ""))
		.context(error::Base64)
		.and_then(|s| {
			toml::from_slice::<toml::value::Table>(&s).context(error::Toml)
		})
		.and_then(process::process_from_table)
		.map(|p| p.into_iter())
}

pub async fn update(
	db: &Arc<RwLock<DB>>,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
) -> Result<()> {
	for repo in github_bot.repositories().await?.iter() {
		if let Ok(repo_projects) = github_bot.projects(&repo.name).await {
			if let Ok(contents) =
				github_bot.contents(&repo.name, "Process.toml").await
			{
				if let Ok(process) = process_from_contents(contents) {
					let projects_with_process = process
						.into_iter()
						.map(|(key, process_info)| {
							(
								repo_projects
									.iter()
									.find(|rp| rp.name == key)
									.cloned(),
								process_info,
							)
						})
						.collect::<Vec<(Option<github::Project>, process::ProcessInfo)>>(
						);

					// projects in Process.toml are useless if they do not match a project
					// in the repo
					if projects_with_process.len() > 0 {
						for issue in github_bot
							.repository_issues(&repo)
							.await?
							.iter()
							.skip(1)
						{
							// if issue.pull_request.is_some() then this issue is a pull
							// request, which we treat differently
							if issue.pull_request.is_none() {
								handle_issue(
									db,
									github_bot,
									matrix_bot,
									core_devs,
									github_to_matrix,
									projects_with_process.as_ref(),
									&repo,
									&issue,
								)
								.await?;
							}
						}

						for pr in github_bot.pull_requests(&repo).await? {
							handle_pull_request(
								db,
								github_bot,
								matrix_bot,
								core_devs,
								github_to_matrix,
								projects_with_process.as_ref(),
								&repo,
								&pr,
							)
							.await?;
						}
					} else {
						// TODO notify projects in Process.toml do not match projects in repo
					}
				} else {
					// TODO notify Process.toml malformed or missing fields
				}
			} else {
				// no Process.toml so ignore and continue
			}
		} else {
			log::error!(
				"Error getting projects for repo '{repo_name}'. They may be disabled.",
				repo_name = repo.name
			);
		}
	}
	Ok(())
}
