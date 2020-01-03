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
use itertools::Itertools;
use rocksdb::DB;
use snafu::{
	GenerateBacktrace,
	ResultExt,
};
use std::collections::HashMap;
use std::time::{
	Duration,
	SystemTime,
};

fn issue_project(issue: &github::Issue, github_bot: &GithubBot) -> Result<Option<github::Project>> {
	let repo = &issue.repository;
	let issue_number = error::unwrap_field(issue.number)?;
	github_bot
		.issue_events(&repo.name, issue_number)
		.map(|issue_events| {
			issue_events
				.iter()
				.sorted_by_key(|ie| ie.created_at)
				.rev()
				.find(|issue_event| {
					issue_event.event == Some(github::Event::AddedToProject)
						|| issue_event.event == Some(github::Event::RemovedFromProject)
				})
				.and_then(|issue_event| {
					if issue_event.event == Some(github::Event::AddedToProject) {
						issue_event
							.project_card
							.as_ref()
							.and_then(|card| card.project_url.as_ref())
					} else {
						None
					}
				})
				.and_then(|project_url| github_bot.get(project_url).ok())
		})
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
	let author = &issue.user;
	let repo = &issue.repository;
	let author_is_owner = repo.owner.id == author.id;
	let author_is_whitelisted = repo
		.whitelist()
		.iter()
		.find(|w| w.id == author.id)
		.is_some();
	let author_is_core = core_devs.iter().find(|u| u.id == author.id).is_some();

	match issue_project(issue, github_bot)? {
		None => {}
		Some(project) => {}
	}
	Ok(())
}
