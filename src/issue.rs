use crate::db::*;
use crate::{
	constants::*,
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
	OptionExt,
	ResultExt,
};
use std::collections::HashMap;
use std::time::{
	Duration,
	SystemTime,
};

fn issue_actor_and_project(
	issue: &github::Issue,
	github_bot: &GithubBot,
) -> Result<Option<(github::User, github::Project)>> {
	let repo = &issue.repository;
	let issue_number = issue.number.context(error::MissingData)?;
	github_bot
		.issue_events(&repo.name, issue_number)
		.map(|issue_events| {
			issue_events
				.iter()
				.sorted_by_key(|ie| ie.created_at)
				.rev()
				.find(|issue_event| {
					issue_event.event == Some(github::Event::AddedToProject)
						|| issue_event.event
							== Some(github::Event::RemovedFromProject)
				})
				.and_then(|issue_event| {
					if issue_event.event == Some(github::Event::AddedToProject)
					{
						issue_event.project_card.as_ref().and_then(|card| {
							card.project_url.as_ref().map(|project_url| {
								(issue_event.actor.clone(), project_url)
							})
						})
					} else {
						None
					}
				})
				.and_then(|(actor, project_url)| {
					github_bot
						.get(project_url)
						.ok()
						.map(|project| (actor, project))
				})
		})
}

fn author_core_no_project(
	db_entry: &mut DbEntry,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	default_channel_id: &str,
	since: Option<Duration>,
) -> Result<DbEntryState> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	let ticks = since
		.map(|elapsed| elapsed.as_secs() / ISSUE_NO_PROJECT_CORE_PING_PERIOD);

	Ok(match ticks {
		None => {
			db_entry.issue_no_project_ping = Some(SystemTime::now());
			matrix_bot.send_public_message(
				default_channel_id,
				&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
			);
			DbEntryState::Update
		}
		Some(0) => DbEntryState::DoNothing,
		Some(i) => {
			if i == ISSUE_NO_PROJECT_ACTION_AFTER_NPINGS {
				// If after 3 days there is still no project
				// attached, move the issue to Core Sorting
				// repository
				github_bot.close_issue(&issue.repository.name, issue_id);
				let params = serde_json::json!({
						"title": issue.title,
						"body": issue.body.as_ref().unwrap_or(&"".to_owned())
				});
				github_bot.create_issue(CORE_SORTING_REPO, params);
				DbEntryState::Delete
			} else if (db_entry.issue_no_project_npings) < i {
				db_entry.issue_no_project_npings = i;
				matrix_bot.send_public_message(
					default_channel_id,
					&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
				);
				DbEntryState::Update
			} else {
				DbEntryState::DoNothing
			}
		}
	})
}

fn author_non_core_no_project(
	db_entry: &mut DbEntry,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	default_channel_id: &str,
	since: Option<Duration>,
) -> Result<DbEntryState> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	let ticks = since.map(|elapsed| {
		elapsed.as_secs() / ISSUE_NO_PROJECT_NON_CORE_PING_PERIOD
	});

	Ok(match ticks {
		None => {
			// send a message to the "Core Developers" room
			// on Riot with the title of the issue and a link.
			db_entry.issue_no_project_ping = Some(SystemTime::now());
			matrix_bot.send_public_message(
				default_channel_id,
				&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
			);
			DbEntryState::Update
		}
		Some(0) => DbEntryState::DoNothing,
		_ => {
			// If after 15 minutes there is still no project
			// attached, move the issue to Core Sorting
			// repository.
			github_bot.close_issue(&issue.repository.name, issue_id);
			let params = serde_json::json!({
					"title": issue.title,
					"body": issue.body.as_ref().unwrap_or(&"".to_owned())
			});
			github_bot.create_issue(CORE_SORTING_REPO, params);
			DbEntryState::Delete
		}
	})
}

pub fn handle_issue(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: Option<&project::Projects>,
	issue: &github::Issue,
	default_channel_id: &str,
) -> Result<()> {
	// TODO: handle multiple projects in a single repo

	let db_key = format!("{}", issue_id).into_bytes();
	let mut db_entry = DbEntry::new_or_with_key(db, db_key);

	let author_is_core =
		core_devs.iter().find(|u| u.id == issue.user.id).is_some();

	match if projects.map_or(true, |p| p.0.is_empty()) {
		unimplemented!()
	} else {
		match issue_actor_and_project(issue, github_bot)? {
			None => {
				let since = db_entry
					.issue_no_project_ping
					.and_then(|ping| ping.elapsed().ok());

				if author_is_core {
					author_core_no_project(
						&mut db_entry,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?
				} else {
					author_non_core_no_project(
						&mut db_entry,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?
				}
			}
			Some((actor, project)) => unimplemented!(),
		}
	} {
		DbEntryState::Delete => {
			db_entry.delete(db)?;
		}
		DbEntryState::Update => {
			db_entry.update(db)?;
		}
		_ => {}
	}

	Ok(())
}
