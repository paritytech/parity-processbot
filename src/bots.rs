use rocksdb::DB;
use std::collections::HashMap;

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
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	default_channel_id: &str,
) -> Result<()> {
	for repo in github_bot.repositories().await? {
		let repo_projects = github_bot.projects(&repo.name).await?;

		// projects in Projects.toml are useless if they do not match a project
		// in the repo
		let projects = github_bot
			.contents(&repo.name, "Projects.toml")
			.await
			.ok()
			.and_then(projects_from_contents)
			.map(|p| {
				p.filter_map(|(key, project_info)| {
					repo_projects
						.iter()
						.find(|rp| rp.name == key)
						.map(|rp| (rp.clone(), project_info))
				})
				.collect::<Vec<(github::Project, project_info::ProjectInfo)>>()
			});

		for issue in github_bot.issues(&repo).await? {
			handle_issue(
				db,
				github_bot,
				matrix_bot,
				core_devs,
				github_to_matrix,
				projects.as_ref(),
				&issue,
				default_channel_id,
			)
			.await?;
		}

		for pr in github_bot.pull_requests(&repo).await? {
			handle_pull_request(
				db,
				github_bot,
				matrix_bot,
				core_devs,
				github_to_matrix,
				projects.as_ref(),
				&pr,
			)
			.await?;
		}
	}
	Ok(())
}
