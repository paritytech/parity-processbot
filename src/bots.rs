use rocksdb::DB;
use std::collections::HashMap;

use crate::{
	github,
	github_bot::GithubBot,
	matrix_bot::MatrixBot,
	project_info,
	pull_request::handle_pull_request,
	Result,
};

pub fn update(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	_default_channel_id: &str,
) -> Result<()> {
	for repo in github_bot.repositories()? {
		let repo_projects = github_bot.projects(&repo.name)?;

		// projects in Projects.toml are useless if they do not match a project
		// in the repo
		let projects = github_bot
			.contents(&repo.name, "Projects.toml")
			.ok()
			.and_then(|c| base64::decode(&c.content.replace("\n", "")).ok())
			.and_then(|s| toml::from_slice::<toml::value::Table>(&s).ok())
			.map(project_info::Projects::from)
			.map(|projects| {
				projects
					.0
					.into_iter()
					.filter_map(|(key, project_info)| {
						repo_projects
							.iter()
							.find(|rp| rp.name == key)
							.map(|rp| (rp.clone(), project_info))
					})
					.collect::<Vec<(github::Project, project_info::ProjectInfo)>>(
					)
			});

		let prs = github_bot.pull_requests(&repo)?;
		for pr in prs {
			handle_pull_request(
				db,
				github_bot,
				matrix_bot,
				core_devs,
				github_to_matrix,
				projects.as_ref(),
				&pr,
			)?;
		}
	}
	Ok(())
}
