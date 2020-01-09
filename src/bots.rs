use rocksdb::DB;
use std::collections::HashMap;

use crate::{
	github, github_bot::GithubBot, matrix_bot::MatrixBot, project,
	pull_request::handle_pull_request, Result,
};

pub async fn update(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
) -> Result<()> {
	for repo in github_bot.repositories().await? {
		let projects = github_bot
			.contents(&repo.name, "Projects.toml")
			.await
			.ok()
			.and_then(|c| base64::decode(&c.content.replace("\n", "")).ok())
			.and_then(|s| toml::from_slice::<toml::value::Table>(&s).ok())
			.map(project::Projects::from);

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
