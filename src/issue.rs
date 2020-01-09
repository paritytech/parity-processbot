use crate::db::*;
use crate::{
	constants::*, duration_ticks::DurationTicks, error, github,
	github_bot::GithubBot, matrix, matrix_bot::MatrixBot, project_info, Result,
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
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	issue: &github::Issue,
	project: &github::Project,
	actor: &github::User,
) -> Result<()> {
	if let Some(backlog_column) = project
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
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Confirmed,
				actor_login: actor.login.clone(),
				project_column_id: backlog_column.id,
			}),
			db,
		)?;
		github_bot.create_project_card(
			backlog_column.id,
			issue.id.context(error::MissingData)?,
			github::ProjectCardContentType::Issue,
		)?;
	} else {
		// TODO warn that the project lacks a backlog column
	}
	Ok(())
}

fn author_core_no_project(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	default_channel_id: &str,
	since: Option<Duration>,
) -> Result<()> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	let ticks = since.ticks(ISSUE_NO_PROJECT_CORE_PING_PERIOD);

	match ticks {
		None => {
			local_state
				.update_issue_no_project_ping(Some(SystemTime::now()), db)?;
			matrix_bot.send_public_message(
				default_channel_id,
				&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
			)?;
		}
		Some(0) => {}
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
				github_bot.create_issue(CORE_SORTING_REPO, &params)?;
				local_state.delete(db)?;
			} else if (local_state.issue_no_project_npings()) < i {
				local_state.update_issue_no_project_npings(i, db)?;
				matrix_bot.send_public_message(
					default_channel_id,
					&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
				)?;
			} else {
			}
		}
	}
	Ok(())
}

fn author_unknown_no_project(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	default_channel_id: &str,
	since: Option<Duration>,
) -> Result<()> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	let ticks = since.ticks(ISSUE_NO_PROJECT_NON_CORE_PING_PERIOD);

	match ticks {
		None => {
			// send a message to the "Core Developers" room
			// on Riot with the title of the issue and a link.
			local_state
				.update_issue_no_project_ping(Some(SystemTime::now()), db)?;
			matrix_bot.send_public_message(
				default_channel_id,
				&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
			)?;
		}
		Some(0) => {}
		_ => {
			// If after 15 minutes there is still no project
			// attached, move the issue to Core Sorting
			// repository.
			github_bot.close_issue(&issue.repository.name, issue_id)?;
			let params = serde_json::json!({
					"title": issue.title,
					"body": issue.body.as_ref().unwrap_or(&"".to_owned())
			});
			github_bot.create_issue(CORE_SORTING_REPO, &params)?;
			local_state.delete(db)?;
		}
	}
	Ok(())
}

fn author_non_admin_project_state_none(
	db: &DB,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
	actor: &github::User,
) -> Result<()> {
	if let Some(room_id) = &project_info.matrix_room_id {
		local_state
			.update_issue_confirm_project_ping(Some(SystemTime::now()), db)?;
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Unconfirmed,
				actor_login: actor.login.clone(),
				project_column_id: project_column.id,
			}),
			db,
		)?;
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
	} else if let Some(_owner) = &project_info.owner {
		// TODO notify project owner
	} else {
		// TODO notify default matrix room
	}
	Ok(())
}

fn author_non_admin_project_state_unconfirmed(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
	actor: &github::User,
) -> Result<()> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	let project_html_url =
		project.html_url.as_ref().context(error::MissingData)?;

	let issue_project =
		local_state.issue_project().expect("has to be Some here");
	let unconfirmed_id = issue_project.project_column_id;

	if project_column.id != unconfirmed_id {
		local_state
			.update_issue_confirm_project_ping(Some(SystemTime::now()), db)?;
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Unconfirmed,
				actor_login: actor.login.clone(),
				project_column_id: project_column.id,
			}),
			db,
		)?;
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
		} else if let Some(_owner) = &project_info.owner {
			// TODO notify project owner
		} else {
			// TODO notify default matrix room
		}
	} else {
		let ticks = local_state
			.issue_confirm_project_ping()
			.and_then(|t| t.elapsed().ok())
			.ticks(ISSUE_UNCONFIRMED_PROJECT_PING_PERIOD);

		match ticks.expect("don't know how long to wait for confirmation; shouldn't ever allow issue_project_state to be set without updating issue_confirm_project_ping") {
			0 => {}
			_ => {
				// confirmation timeout. delete project card and reattach last
				// confirmed if possible
				local_state.update_issue_confirm_project_ping(None, db)?;
				local_state.update_issue_project(
					local_state.last_confirmed_issue_project().cloned(),
					db,
				)?;
				github_bot.delete_project_card(unconfirmed_id)?;
				if let Some(prev_column_id) =
					local_state.issue_project().map(|p| p.project_column_id)
				{
					// reattach the last confirmed project
					github_bot.create_project_card(
						prev_column_id,
						issue_id,
						github::ProjectCardContentType::Issue,
					)?;
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
			}
		}
	}
	Ok(())
}

fn author_non_admin_project_state_denied(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
	actor: &github::User,
) -> Result<()> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	let project_html_url =
		project.html_url.as_ref().context(error::MissingData)?;
	let denied_id = local_state.issue_project().unwrap().project_column_id;

	if project_column.id != denied_id {
		local_state
			.update_issue_confirm_project_ping(Some(SystemTime::now()), db)?;
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Unconfirmed,
				actor_login: actor.login.clone(),
				project_column_id: project_column.id,
			}),
			db,
		)?;
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
		} else if let Some(_owner) = &project_info.owner {
			// TODO notify project owner
		} else {
			// TODO notify default matrix room
		}
	} else {
		local_state.update_issue_confirm_project_ping(None, db)?;
		local_state.update_issue_project(
			local_state.last_confirmed_issue_project().cloned(),
			db,
		)?;
		if let Some(prev_column_id) =
			local_state.issue_project().map(|p| p.project_column_id)
		{
			// reattach the last confirmed project
			github_bot.create_project_card(
				prev_column_id,
				issue_id,
				github::ProjectCardContentType::Issue,
			)?;
		}
	}
	if let Some(matrix_id) = github_to_matrix
		.get(&local_state.issue_project().unwrap().actor_login)
		.and_then(|matrix_id| matrix::parse_id(matrix_id))
	{
		matrix_bot.send_private_message(
			&matrix_id,
			&ISSUE_REVERT_PROJECT_NOTIFICATION.replace("{1}", &issue_html_url),
		)?;
	} else if let Some(_owner) = &project_info.owner {
		// TODO notify project owner
	} else {
		// TODO notify default matrix room
	}
	Ok(())
}

fn author_non_admin_project_state_confirmed(
	db: &DB,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	_default_channel_id: &str,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	project_info: &project_info::ProjectInfo,
	actor: &github::User,
) -> Result<()> {
	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	let project_html_url =
		project.html_url.as_ref().context(error::MissingData)?;

	let confirmed_id = local_state.issue_project().unwrap().project_column_id;

	let confirmed_matches_last = local_state
		.last_confirmed_issue_project()
		.map(|proj| proj.project_column_id == confirmed_id)
		.unwrap_or(false);

	if !confirmed_matches_last {
		local_state.update_issue_confirm_project_ping(None, db)?;
		local_state.update_last_confirmed_issue_project(
			local_state.issue_project().cloned(),
			db,
		)?;
	}

	if project_column.id != confirmed_id {
		// project has been changed since
		// the confirmation
		local_state
			.update_issue_confirm_project_ping(Some(SystemTime::now()), db)?;
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Unconfirmed,
				actor_login: actor.login.clone(),
				project_column_id: project_column.id,
			}),
			db,
		)?;
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
		} else if let Some(_owner) = &project_info.owner {
			// TODO notify project owner
		} else {
			// TODO notify default matrix room
		}
	}
	Ok(())
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

	let db_key = issue_id.to_le_bytes().to_vec();
	let mut local_state = LocalState::get_or_new(db, db_key)?;

	let author_is_core = core_devs.iter().any(|u| u.id == issue.user.id);

	if projects.map_or(true, |p| p.is_empty()) {
		// there are no projects matching those listed in Projects.toml so do nothing
	} else {
		let projects = projects.expect("just confirmed above");
		match issue_actor_and_project_card(issue, github_bot)? {
			None => {
				let since = local_state
					.issue_no_project_ping()
					.and_then(|ping| ping.elapsed().ok());
				let admin_of = projects
					.iter()
					.find(|(_, p)| p.is_admin(&issue.user.login));

				if projects.len() == 1 && admin_of.is_some() {
					// repo contains only one project and the author is admin
					// so we can attach it with high confidence
					let (project, _) = admin_of.expect("checked above");
					author_admin_attach_only_project(
						db,
						&mut local_state,
						github_bot,
						issue,
						&project,
						&issue.user,
					)?;
				} else if author_is_core
					|| projects
						.iter()
						.find(|(_, p)| p.is_admin(&issue.user.login))
						.is_some()
				{
					// author is a core developer or admin of at least one
					// project in the repo
					author_core_no_project(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?;
				} else {
					// author is neither core developer nor admin
					author_unknown_no_project(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?;
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

						match local_state.issue_project().map(|p| p.state) {
							None => author_non_admin_project_state_none(
								db,
								&mut local_state,
								matrix_bot,
								issue,
								default_channel_id,
								&project,
								&project_column,
								&project_info,
								&actor,
							)?,
							Some(IssueProjectState::Unconfirmed) => {
								author_non_admin_project_state_unconfirmed(
									db,
									&mut local_state,
									github_bot,
									matrix_bot,
									github_to_matrix,
									issue,
									default_channel_id,
									&project,
									&project_column,
									&project_info,
									&actor,
								)?
							}
							Some(IssueProjectState::Denied) => {
								author_non_admin_project_state_denied(
									db,
									&mut local_state,
									github_bot,
									matrix_bot,
									github_to_matrix,
									issue,
									default_channel_id,
									&project,
									&project_column,
									&project_info,
									&actor,
								)?
							}
							Some(IssueProjectState::Confirmed) => {
								author_non_admin_project_state_confirmed(
									db,
									&mut local_state,
									matrix_bot,
									issue,
									default_channel_id,
									&project,
									&project_column,
									&project_info,
									&actor,
								)?
							}
						};
					} else {
						// actor is admin so allow any change
					}
				} else {
					// no key in in Projects.toml matches the project name
					// TODO notification here
				}
			}
		}
	}

	Ok(())
}
