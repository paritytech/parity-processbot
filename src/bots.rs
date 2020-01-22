use parking_lot::RwLock;
use rocksdb::DB;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
	github, github_bot::GithubBot, issue::handle_issue, matrix_bot::MatrixBot,
	project_info, pull_request::handle_pull_request, Result,
};

fn projects_from_contents(
	c: github::Contents,
) -> Option<impl Iterator<Item = (String, project_info::ProjectInfo)>> {
	base64::decode(&c.content.replace("\n", ""))
		.ok()
		.and_then(|s| toml::from_slice::<toml::value::Table>(&s).ok())
		.map(project_info::projects_from_table)
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
			// projects in Projects.toml are useless if they do not match a project
			// in the repo
			let projects = github_bot
				.contents(&repo.name, "Projects.toml")
				.await
				.ok()
				.and_then(projects_from_contents)
				.into_iter()
				.flat_map(|p| p)
				.map(|(key, project_info)| {
					(
						repo_projects.iter().find(|rp| rp.name == key).cloned(),
						project_info,
					)
				})
				.collect::<Vec<(Option<github::Project>, project_info::ProjectInfo)>>(
				);

			if projects.len() > 0 {
				for issue in
					github_bot.repository_issues(&repo).await?.iter().skip(1)
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
							projects.as_ref(),
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
						projects.as_ref(),
						&repo,
						&pr,
					)
					.await?;
				}
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
