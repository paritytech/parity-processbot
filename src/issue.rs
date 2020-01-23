use crate::db::*;
use crate::local_state::*;
use crate::{
	constants::*, duration_ticks::DurationTicks, error, github,
	github_bot::GithubBot, matrix, matrix_bot::MatrixBot, process, Result,
};
use parking_lot::RwLock;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Return the project card attached to an issue, if there is one, and the user who attached it
pub async fn issue_actor_and_project_card(
	repo_name: &str,
	issue_number: i64,
	github_bot: &GithubBot,
) -> Result<Option<(github::User, github::ProjectCard)>> {
	Ok(github_bot
		.active_project_event(repo_name, issue_number)
		.await?
		.and_then(|mut issue_event| {
			issue_event
				.project_card
				.take()
				.map(|card| (issue_event.actor, card))
		}))
}

async fn author_special_attach_only_project(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	(project, process_info): (&github::Project, &process::ProcessInfo),
	actor: &github::User,
) -> Result<()> {
	if let Some(backlog_column) = github_bot
		.project_column_by_name(project, PROJECT_BACKLOG_COLUMN_NAME)
		.await?
	{
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Confirmed,
				actor_login: actor.login.clone(),
				project_column_id: backlog_column.id,
			}),
			db,
		)?;
		github_bot
			.create_project_card(
				backlog_column.id,
				issue.id.context(error::MissingData)?,
				github::ProjectCardContentType::Issue,
			)
			.await?;
	} else {
		log::warn!(
			"{project_url} needs a backlog column",
			project_url =
				project.html_url.as_ref().context(error::MissingData)?,
		);
		matrix_bot.send_to_room_or_default(
			process_info.matrix_room_id.as_ref(),
			&PROJECT_NEEDS_BACKLOG_MESSAGE.replace(
				"{1}",
				project.html_url.as_ref().context(error::MissingData)?,
			),
		)?;
	}
	Ok(())
}

async fn author_core_no_project(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	since: Option<Duration>,
) -> Result<()> {
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	let ticks = since.ticks(ISSUE_NO_PROJECT_CORE_PING_PERIOD);
	match ticks {
		None => {
			local_state
				.update_issue_no_project_ping(Some(SystemTime::now()), db)?;
			matrix_bot.send_to_default(
				&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
			)?;
		}
		Some(0) => {}
		Some(i) => {
			if i >= ISSUE_NO_PROJECT_ACTION_AFTER_NPINGS {
				// If after 3 days there is still no project
				// attached, move the issue to Core Sorting
				// repository
				github_bot
					.close_issue(
						&issue
							.repository
							.as_ref()
							.context(error::MissingData)?
							.name,
						issue.number.context(error::MissingData)?,
					)
					.await?;
				github_bot
					.create_issue(
						CORE_SORTING_REPO,
						issue.title.as_ref().unwrap_or(&"".to_owned()),
						issue.body.as_ref().unwrap_or(&"".to_owned()),
						&issue
							.assignee
							.as_ref()
							.map(|a| a.login.as_ref())
							.unwrap_or(String::new().as_ref()),
					)
					.await?;
				local_state.delete(db, &local_state.key)?;
			} else {
				local_state.update_issue_no_project_npings(i, db)?;
				matrix_bot.send_to_default(
					&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
				)?;
			}
		}
	}
	Ok(())
}

async fn author_unknown_no_project(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	since: Option<Duration>,
) -> Result<()> {
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	let ticks = since.ticks(ISSUE_NO_PROJECT_NON_CORE_PING_PERIOD);

	match ticks {
		None => {
			// send a message to the "Core Developers" room
			// on Riot with the title of the issue and a link.
			local_state
				.update_issue_no_project_ping(Some(SystemTime::now()), db)?;
			matrix_bot.send_to_default(
				&ISSUE_NO_PROJECT_MESSAGE.replace("{1}", issue_html_url),
			)?;
		}
		Some(0) => {}
		_ => {
			// If after 15 minutes there is still no project
			// attached, move the issue to Core Sorting
			// repository.
			github_bot
				.close_issue(
					&issue
						.repository
						.as_ref()
						.context(error::MissingData)?
						.name,
					issue.number.context(error::MissingData)?,
				)
				.await?;
			github_bot
				.create_issue(
					CORE_SORTING_REPO,
					issue.title.as_ref().unwrap_or(&"".to_owned()),
					issue.body.as_ref().unwrap_or(&"".to_owned()),
					&issue
						.assignee
						.as_ref()
						.map(|a| a.login.as_ref())
						.unwrap_or(String::new().as_ref()),
				)
				.await?;
			local_state.delete(db, &local_state.key)?;
		}
	}
	Ok(())
}

fn author_non_special_project_state_none(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	process_info: &process::ProcessInfo,
	actor: &github::User,
) -> Result<()> {
	if let Some(room_id) = &process_info.matrix_room_id {
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
		matrix_bot.send_to_room(
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
	} else if let Some(_owner) = &process_info.owner_or_delegate() {
		// TODO notify project owner
	} else {
		// TODO notify default matrix room
	}
	Ok(())
}

async fn author_non_special_project_state_unconfirmed(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	issue: &github::Issue,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	process_info: &process::ProcessInfo,
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
		if let Some(room_id) = &process_info.matrix_room_id {
			matrix_bot.send_to_room(
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
		} else if let Some(_owner) = &process_info.owner_or_delegate() {
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
				github_bot.delete_project_card(unconfirmed_id).await?;
				if let Some(prev_column_id) =
					local_state.issue_project().map(|p| p.project_column_id)
				{
					// reattach the last confirmed project
					github_bot.create_project_card(
						prev_column_id,
						issue_id,
						github::ProjectCardContentType::Issue,
					).await?;
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

async fn author_non_special_project_state_denied(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	issue: &github::Issue,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	process_info: &process::ProcessInfo,
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
		if let Some(room_id) = &process_info.matrix_room_id {
			matrix_bot.send_to_room(
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
		} else if let Some(_owner) = &process_info.owner_or_delegate() {
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
			github_bot
				.create_project_card(
					prev_column_id,
					issue_id,
					github::ProjectCardContentType::Issue,
				)
				.await?;
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
	} else if let Some(_owner) = &process_info.owner_or_delegate() {
		// TODO notify project owner
	} else {
		// TODO notify default matrix room
	}
	Ok(())
}

fn author_non_special_project_state_confirmed(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	issue: &github::Issue,
	project: &github::Project,
	project_column: &github::ProjectColumn,
	process_info: &process::ProcessInfo,
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
		if let Some(room_id) = &process_info.matrix_room_id {
			matrix_bot.send_to_room(
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
		} else if let Some(_owner) = &process_info.owner_or_delegate() {
			// TODO notify project owner
		} else {
			// TODO notify default matrix room
		}
	}
	Ok(())
}

pub async fn handle_issue(
	db: &Arc<RwLock<DB>>,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: &[(Option<github::Project>, process::ProcessInfo)],
	repo: &github::Repository,
	issue: &github::Issue,
) -> Result<()> {
	let issue_id = issue.id.context(error::MissingData)?;

	let db_key = issue_id.to_le_bytes().to_vec();
	let mut local_state = LocalState::get_or_default(db, db_key)?;

	let author_is_core = core_devs
		.iter()
		.any(|u| issue.user.as_ref().map_or(false, |user| u.id == user.id));

	if projects.is_empty() {
		// there are no projects matching those listed in Process.toml so do nothing
	} else {
		match issue_actor_and_project_card(
			&repo.name,
			issue.number.context(error::MissingData)?,
			github_bot,
		)
		.await?
		{
			None => {
				log::info!(
                    "Handling issue '{issue_title}' with no project in repo '{repo_name}'",
                    issue_title = issue.title.as_ref().unwrap_or(&"".to_owned()),
                    repo_name = repo.name
                );

				let since = local_state
					.issue_no_project_ping()
					.and_then(|ping| ping.elapsed().ok());
				let special_of_project = projects
					.iter()
					.find(|(_, p)| {
						issue
							.user
							.as_ref()
							.map_or(false, |user| p.is_special(&user.login))
					})
					.and_then(|(p, pi)| p.as_ref().map(|p| (p, pi)));

				if projects.len() == 1 && special_of_project.is_some() {
					// repo contains only one project and the author is special
					// so we can attach it with high confidence
					author_special_attach_only_project(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						issue,
						special_of_project.expect("checked above"),
						&issue.user.as_ref().expect("checked above"),
					)
					.await?;
				} else if author_is_core
					|| projects
						.iter()
						.find(|(_, p)| {
							issue
								.user
								.as_ref()
								.map_or(false, |user| p.is_special(&user.login))
						})
						.is_some()
				{
					// author is a core developer or special of at least one
					// project in the repo
					author_core_no_project(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						issue,
						since,
					)
					.await?;
				} else {
					// author is neither core developer nor special
					author_unknown_no_project(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						issue,
						since,
					)
					.await?;
				}
			}
			Some((actor, card)) => {
				let project: github::Project =
					github_bot.project(&card).await?;
				let project_column: github::ProjectColumn =
					github_bot.project_column(&card).await?;

				log::info!(
                        "Handling issue '{issue_title}' in project '{project_name}' in repo '{repo_name}'",
                        issue_title = issue.title.as_ref().unwrap_or(&"".to_owned()),
                        project_name = project.name,
                        repo_name = repo.name
                );

				if let Some(process_info) = projects
					.iter()
					.find(|(p, _)| {
						p.as_ref().map_or(false, |p| &project.name == &p.name)
					})
					.map(|(_, p)| p)
				{
					if !process_info.is_special(&actor.login) {
						// TODO check if confirmation has confirmed/denied.
						// requires parsing messages in project room

						match local_state.issue_project().map(|p| p.state) {
							None => author_non_special_project_state_none(
								db,
								&mut local_state,
								matrix_bot,
								issue,
								&project,
								&project_column,
								&process_info,
								&actor,
							)?,
							Some(IssueProjectState::Unconfirmed) => {
								author_non_special_project_state_unconfirmed(
									db,
									&mut local_state,
									github_bot,
									matrix_bot,
									github_to_matrix,
									issue,
									&project,
									&project_column,
									&process_info,
									&actor,
								)
								.await?
							}
							Some(IssueProjectState::Denied) => {
								author_non_special_project_state_denied(
									db,
									&mut local_state,
									github_bot,
									matrix_bot,
									github_to_matrix,
									issue,
									&project,
									&project_column,
									&process_info,
									&actor,
								)
								.await?
							}
							Some(IssueProjectState::Confirmed) => {
								author_non_special_project_state_confirmed(
									db,
									&mut local_state,
									matrix_bot,
									issue,
									&project,
									&project_column,
									&process_info,
									&actor,
								)?
							}
						};
					} else {
						// actor is special so allow any change
					}
				} else {
					// no key in in Process.toml matches the project name
					// TODO notification here
				}
			}
		}
	}
	Ok(())
}
