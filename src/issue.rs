use crate::db::*;
use crate::{
	error,
	github,
	github_bot::GithubBot,
	matrix,
	matrix_bot::MatrixBot,
	project,
	Result,
};
use rocksdb::DB;
use snafu::ResultExt;
use std::collections::HashMap;
use std::time::{
	Duration,
	SystemTime,
};

pub fn issue_project(
	issue: &github::Issue,
	repo: &github::Repository,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	project_info: Option<&project::ProjectInfo>,
) -> Result<Option<github::Project>> {
	github_bot
		.issue_events(&repo.name, issue.number)
		.map(|issue_events| {
			issue_events.iter().filter_map(|issue_event| {
				if issue_event.event == github::Event::AddedToProject
					|| issue_event.event == github::Event::RemovedFromProject
				{
					Some(issue_event)
				} else {
					None
				}
			});
		});
	Ok(None)
}

pub fn handle_issue(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	project_info: Option<&project::ProjectInfo>,
	issue: &github::Issue,
) -> Result<()> {
	Ok(())
}
