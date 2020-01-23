use parking_lot::RwLock;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
	constants::*, error, github, github_bot::GithubBot, issue::handle_issue,
	matrix_bot::MatrixBot, process, pull_request::handle_pull_request, Result,
};

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
				if let Ok(process) = process::process_from_contents(contents) {
					// projects in Process.toml are useless if they do not match a project
					// in the repo
					let projects_with_process = process
						.into_iter()
						.map(
							|(key, process_info): (
								String,
								process::ProcessInfo,
							)| {
								(
									repo_projects
										.iter()
										.find(|rp| rp.name == key)
										.cloned(),
									process_info,
								)
							},
						)
						.collect::<Vec<(Option<github::Project>, process::ProcessInfo)>>(
						);

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
						matrix_bot.send_to_default(
							&MISMATCHED_PROCESS_FILE.replace(
								"{1}",
								&repo
									.html_url
									.as_ref()
									.context(error::MissingData)?,
							),
						)?;
					}
				} else {
					matrix_bot.send_to_default(
						&MALFORMED_PROCESS_FILE.replace(
							"{1}",
							&repo
								.html_url
								.as_ref()
								.context(error::MissingData)?,
						),
					)?;
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
