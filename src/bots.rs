use rocksdb::DB;
use std::collections::HashMap;

use crate::{
	github,
	github_bot::GithubBot,
	matrix_bot::MatrixBot,
	project,
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
		let projects = github_bot
			.contents(&repo.name, "Projects.toml")
			.ok()
			.and_then(|c| base64::decode(&c.content.replace("\n", "")).ok())
			.and_then(|s| toml::from_slice::<toml::value::Table>(&s).ok())
			.map(project::Projects::from);

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
