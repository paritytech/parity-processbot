use crate::db::*;
use crate::{
	constants::*, error, github, github_bot::GithubBot, matrix,
	matrix_bot::MatrixBot, project_info, Result,
};
use itertools::Itertools;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

fn issue_actor_and_project_card(
	issue: &github::Issue,
	github_bot: &GithubBot,
) -> Result<Option<(github::User, github::ProjectCard)>> {
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
						issue_event.project_card.as_ref().map(|card| {
							(issue_event.actor.clone(), card.clone())
						})
					} else {
						None
					}
				})
		})
}

fn author_admin_attach_only_project(
	db_entry: &mut DbEntry,
	github_bot: &GithubBot,
	issue: &github::Issue,
	admin_of: &(github::Project, project_info::ProjectInfo),
) -> Result<DbEntryState> {
	Ok(
		if let Some(backlog_column) = admin_of
			.0
			.columns_url
			.as_ref()
			.and_then(|url| github_bot.project_columns(url).ok())
			.and_then(|columns| {
				columns.into_iter().find(|c| {
					c.name
						.as_ref()
						.map(|name| {
							name.to_lowercase()
								== PROJECT_BACKLOG_COLUMN_NAME.to_lowercase()
						})
						.unwrap_or(false)
				})
			}) {
			db_entry.issue_project_state =
				Some(ProjectState::Confirmed(backlog_column.id));
			github_bot.create_project_card(
				backlog_column.id,
				issue.id.context(error::MissingData)?,
				github::ProjectCardContentType::Issue,
			)?;
			DbEntryState::Update
		} else {
			// TODO warn that the project lacks a backlog column
			DbEntryState::DoNothing
		},
	)
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
			)?;
			DbEntryState::Update
		}
		Some(0) => DbEntryState::DoNothing,
		Some(i) => {
			if i == ISSUE_NO_PROJECT_ACTION_AFTER_NPINGS {
				// If after 3 days there is still no project
				// attached, move the issue to Core Sorting
				// repository
				github_bot.close_issue(&issue.repository.name, issue_id)?;
				let params = serde_json::json!({
						"title": issue.title,
						"body": issue.body.as_ref().unwrap_or(&"".to_owned())
				});
				github_bot.create_issue(CORE_SORTING_REPO, params)?;
				DbEntryState::Delete
			} else if (db_entry.issue_no_project_npings) < i {
				db_entry.issue_no_project_npings = i;
				matrix_bot.send_public_message(
					default_channel_id,
					&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
				)?;
				DbEntryState::Update
			} else {
				DbEntryState::DoNothing
			}
		}
	})
}

fn author_unknown_no_project(
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
			)?;
			DbEntryState::Update
		}
		Some(0) => DbEntryState::DoNothing,
		_ => {
			// If after 15 minutes there is still no project
			// attached, move the issue to Core Sorting
			// repository.
			github_bot.close_issue(&issue.repository.name, issue_id)?;
			let params = serde_json::json!({
					"title": issue.title,
					"body": issue.body.as_ref().unwrap_or(&"".to_owned())
			});
			github_bot.create_issue(CORE_SORTING_REPO, params)?;
			DbEntryState::Delete
		}
	})
}

fn author_non_admin_project_state_none(
	db_entry: &mut DbEntry,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
) -> Result<DbEntryState> {
	if let Some(room_id) = &project_info.matrix_room_id {
		db_entry.issue_confirm_project_ping = Some(SystemTime::now());
		db_entry.issue_project_state =
			Some(ProjectState::Unconfirmed(project_column.id));
		matrix_bot.send_public_message(
			&room_id,
			&ISSUE_CONFIRM_PROJECT_MESSAGE
				.replace(
					"{issue_url}",
					issue.html_url.as_ref().context(error::MissingData)?,
				)
				.replace(
					"{project_url}",
					project.html_url.as_ref().context(error::MissingData)?,
				)
				.replace(
					"{issue_id}",
					&format!("{}", issue.id.context(error::MissingData)?),
				)
				.replace(
					"{project_column.id}",
					&format!("{}", project_column.id),
				),
		)?;
	} else {
		// project info should include matrix room
		// id. TODO some kind of notification here
	}
	Ok(DbEntryState::Update)
}

fn author_non_admin_project_state_unconfirmed(
	db_entry: &mut DbEntry,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
	actor: &github::User,
	unconfirmed_id: i64,
) -> Result<DbEntryState> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	let project_html_url =
		project.html_url.as_ref().context(error::MissingData)?;

	Ok(if project_column.id != unconfirmed_id {
		db_entry.issue_confirm_project_ping = Some(SystemTime::now());
		db_entry.issue_project_state =
			Some(ProjectState::Unconfirmed(project_column.id));
		if let Some(room_id) = &project_info.matrix_room_id {
			matrix_bot.send_public_message(
				&room_id,
				&ISSUE_CONFIRM_PROJECT_MESSAGE
					.replace("{issue_url}", issue_html_url)
					.replace("{project_url}", project_html_url)
					.replace("{issue_id}", &format!("{}", issue_id))
					.replace(
						"{project_column.id}",
						&format!("{}", project_column.id),
					),
			)?;
		} else {
			// project info should include matrix
			// room id. TODO some kind of
			// notification here
		}
		DbEntryState::Update
	} else {
		let ticks = db_entry
			.issue_confirm_project_ping
			.and_then(|t| t.elapsed().ok())
			.map(|elapsed| {
				elapsed.as_secs() / ISSUE_UNCONFIRMED_PROJECT_PING_PERIOD
			});

		match ticks {
			None => {
				panic!("don't know how long to wait for confirmation; shouldn't ever allow issue_project_state to be set without updating issue_confirm_project_ping");
			}
			Some(0) => DbEntryState::DoNothing,
			Some(_) => {
				// confirmation timeout. delete project card and reattach last
				// confirmed if possible
				db_entry.issue_confirm_project_ping = None;
				github_bot.delete_project_card(unconfirmed_id)?;
				if let Some(prev_project) = db_entry.last_confirmed_project {
					db_entry.issue_project_state =
						Some(ProjectState::Confirmed(prev_project));
					github_bot.create_project_card(
						prev_project,
						issue_id,
						github::ProjectCardContentType::Issue,
					)?;
				} else {
					db_entry.issue_project_state = None;
				}
				if let Some(matrix_id) = github_to_matrix
					.get(&actor.login)
					.and_then(|matrix_id| matrix::parse_id(matrix_id))
				{
					matrix_bot.send_private_message(
						&matrix_id,
						&ISSUE_REVERT_PROJECT_NOTIFICATION
							.replace("{1}", &issue_html_url),
					)?;
				} else {
					// no matrix id to message
				}
				DbEntryState::Update
			}
		}
	})
}

fn author_non_admin_project_state_denied(
	db_entry: &mut DbEntry,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	issue: &github::Issue,
	_default_channel_id: &str,
	actor: &github::User,
) -> Result<DbEntryState> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	db_entry.issue_confirm_project_ping = None;
	if let Some(prev_project) = db_entry.last_confirmed_project {
		db_entry.issue_project_state =
			Some(ProjectState::Confirmed(prev_project));
		// reattach the last confirmed project
		github_bot.create_project_card(
			prev_project,
			issue_id,
			github::ProjectCardContentType::Issue,
		)?;
	} else {
		db_entry.issue_project_state = None;
	}
	if let Some(matrix_id) = github_to_matrix
		.get(&actor.login)
		.and_then(|matrix_id| matrix::parse_id(matrix_id))
	{
		matrix_bot.send_private_message(
			&matrix_id,
			&ISSUE_REVERT_PROJECT_NOTIFICATION.replace("{1}", &issue_html_url),
		)?;
	} else {
		// no matrix id to message
	}
	Ok(DbEntryState::Update)
}

fn author_non_admin_project_state_confirmed(
	db_entry: &mut DbEntry,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
	confirmed_id: i64,
) -> Result<DbEntryState> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	let project_html_url =
		project.html_url.as_ref().context(error::MissingData)?;

	let confirmed_matches_last = db_entry
		.last_confirmed_project
		.map(|proj_id| proj_id == confirmed_id)
		.unwrap_or(false);

	if !confirmed_matches_last {
		db_entry.issue_confirm_project_ping = None;
		db_entry.last_confirmed_project = Some(confirmed_id);
	}

	if project_column.id != confirmed_id {
		// project has been changed since
		// the confirmation
		db_entry.issue_confirm_project_ping = Some(SystemTime::now());
		db_entry.issue_project_state =
			Some(ProjectState::Unconfirmed(project_column.id));
		if let Some(room_id) = &project_info.matrix_room_id {
			matrix_bot.send_public_message(
				&room_id,
				&ISSUE_CONFIRM_PROJECT_MESSAGE
					.replace("{issue_url}", issue_html_url)
					.replace("{project_url}", project_html_url)
					.replace("{issue_id}", &format!("{}", issue_id))
					.replace(
						"{project_column.id}",
						&format!("{}", project_column.id),
					),
			)?;
		} else {
			// project info should include matrix
			// room id. TODO some kind of
			// notification here
		}
	}
	Ok(DbEntryState::Update)
}

pub fn handle_issue(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: Option<&Vec<(github::Project, project_info::ProjectInfo)>>,
	issue: &github::Issue,
	default_channel_id: &str,
) -> Result<()> {
	// TODO: handle multiple projects in a single repo

	let issue_id = issue.id.context(error::MissingData)?;

	let db_key = format!("{}", issue_id).into_bytes();
	let mut db_entry = DbEntry::new_or_with_key(db, db_key);

	let author_is_core =
		core_devs.iter().find(|u| u.id == issue.user.id).is_some();

	match if projects.map_or(true, |p| p.is_empty()) {
		DbEntryState::DoNothing
	} else {
		let projects = projects.expect("just confirmed above");
		match issue_actor_and_project_card(issue, github_bot)? {
			None => {
				let since = db_entry
					.issue_no_project_ping
					.and_then(|ping| ping.elapsed().ok());
				let admin_of = projects
					.iter()
					.find(|(_, p)| p.is_admin(&issue.user.login));

				if projects.len() == 1 && admin_of.is_some() {
					// repo contains only one project and the author is admin
					// so we can attach it with high confidence
					author_admin_attach_only_project(
						&mut db_entry,
						github_bot,
						issue,
						admin_of.expect("just checked"),
					)?
				} else if author_is_core
					|| projects
						.iter()
						.find(|(_, p)| p.is_admin(&issue.user.login))
						.is_some()
				{
					// author is a core developer or admin of at least one
					// project in the repo
					author_core_no_project(
						&mut db_entry,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?
				} else {
					// author is neither core developer nor admin
					author_unknown_no_project(
						&mut db_entry,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?
				}
			}
			Some((actor, card)) => {
				let project: github::Project = card
					.project_url
					.as_ref()
					.and_then(|url| github_bot.get(url).ok())
					.context(error::MissingData)?;
				let project_column: github::ProjectColumn = card
					.column_url
					.as_ref()
					.and_then(|url| github_bot.get(url).ok())
					.context(error::MissingData)?;

				if let Some(project_info) = projects
					.iter()
					.find(|(p, _)| &p.name == &project.name)
					.map(|(_, p)| p)
				{
					if !project_info.is_admin(&actor.login) {
						// TODO check if confirmation has confirmed/denied.
						// requires parsing messages in project room

						match db_entry.issue_project_state {
							None => author_non_admin_project_state_none(
								&mut db_entry,
								matrix_bot,
								issue,
								default_channel_id,
								&project,
								&project_column,
								&project_info,
							)?,
							Some(ProjectState::Unconfirmed(unconfirmed_id)) => {
								author_non_admin_project_state_unconfirmed(
									&mut db_entry,
									github_bot,
									matrix_bot,
									github_to_matrix,
									issue,
									default_channel_id,
									&project,
									&project_column,
									&project_info,
									&actor,
									unconfirmed_id,
								)?
							}
							Some(ProjectState::Denied(_)) => {
								author_non_admin_project_state_denied(
									&mut db_entry,
									github_bot,
									matrix_bot,
									github_to_matrix,
									issue,
									default_channel_id,
									&actor,
								)?
							}
							Some(ProjectState::Confirmed(confirmed_id)) => {
								author_non_admin_project_state_confirmed(
									&mut db_entry,
									matrix_bot,
									issue,
									default_channel_id,
									&project,
									&project_column,
									&project_info,
									confirmed_id,
								)?
							}
						}
					} else {
						// actor is admin so allow any change
						DbEntryState::DoNothing
					}
				} else {
					// no key in in Projects.toml matches the project name
					// TODO notification here
					DbEntryState::DoNothing
				}
			}
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
