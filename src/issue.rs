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
use snafu::OptionExt;
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

fn author_admin_attach_only_project(
	_db_entry: &mut DbEntry,
	_github_bot: &GithubBot,
	_matrix_bot: &MatrixBot,
	_issue: &github::Issue,
	_default_channel_id: &str,
	_since: Option<Duration>,
) -> Result<DbEntryState> {
	unimplemented!()
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

	let issue_id = issue.id.context(error::MissingData)?;

	let db_key = format!("{}", issue_id).into_bytes();
	let mut db_entry = DbEntry::new_or_with_key(db, db_key);

	let author_is_core =
		core_devs.iter().find(|u| u.id == issue.user.id).is_some();

	match if projects.map_or(true, |p| p.0.is_empty()) {
		unimplemented!()
	} else {
		let projects = projects.expect("just confirmed above");
		match issue_actor_and_project(issue, github_bot)? {
			None => {
				let since = db_entry
					.issue_no_project_ping
					.and_then(|ping| ping.elapsed().ok());

				if projects.0.len() == 1
					&& projects
						.0
						.iter()
						.find(|p| p.1.is_admin(&issue.user.login))
						.is_some()
				{
					// repo contains only one project and the author is admin
					author_admin_attach_only_project(
						&mut db_entry,
						github_bot,
						matrix_bot,
						issue,
						default_channel_id,
						since,
					)?
				} else if author_is_core
					|| projects
						.0
						.iter()
						.find(|p| p.1.is_admin(&issue.user.login))
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
			Some((actor, project)) => {
				if let Some(project_info) = projects.0.get(&project.name) {
					let issue_html_url =
						issue.html_url.as_ref().context(error::MissingData)?;
					let project_html_url = project
						.html_url
						.as_ref()
						.context(error::MissingData)?;
					let project_id =
						project.id.as_ref().context(error::MissingData)?;

					if !project_info.is_admin(&actor.login) {
						// TODO check if confirmation has confirmed/denied.
						// requires parsing messages in project room

						match db_entry.issue_project_state {
							None => {
								if let Some(room_id) =
									&project_info.matrix_room_id
								{
									db_entry.issue_confirm_project_ping =
										Some(SystemTime::now());
									db_entry.issue_project_state = Some(
										ProjectState::Unconfirmed(*project_id),
									);
									matrix_bot.send_public_message(
										&room_id,
										&ISSUE_CONFIRM_PROJECT_MESSAGE
											.replace(
												"{issue_url}",
												issue_html_url,
											)
											.replace(
												"{project_url}",
												project_html_url,
											)
											.replace(
												"{issue_id}",
												&format!("{}", issue_id),
											)
											.replace(
												"{project_id}",
												&format!("{}", project_id),
											),
									)?;
								} else {
									// project info should include matrix room
									// id. TODO some kind of warning here
								}
								DbEntryState::Update
							}
							Some(ProjectState::Unconfirmed(unconfirmed_id)) => {
								if project_id != &unconfirmed_id {
									db_entry.issue_confirm_project_ping =
										Some(SystemTime::now());
									db_entry.issue_project_state = Some(
										ProjectState::Unconfirmed(*project_id),
									);
									if let Some(room_id) =
										&project_info.matrix_room_id
									{
										matrix_bot.send_public_message(
											&room_id,
											&ISSUE_CONFIRM_PROJECT_MESSAGE
												.replace(
													"{issue_url}",
													issue_html_url,
												)
												.replace(
													"{project_url}",
													project_html_url,
												)
												.replace(
													"{issue_id}",
													&format!("{}", issue_id),
												)
												.replace(
													"{project_id}",
													&format!("{}", project_id),
												),
										)?;
									} else {
										// project info should include matrix
										// room id. TODO some kind of warning
										// here
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
											db_entry
												.issue_confirm_project_ping = None;
											if let Some(prev_project) =
												db_entry.last_confirmed_project
											{
												db_entry.issue_project_state =
													Some(
														ProjectState::Confirmed(
															prev_project,
														),
													);
											} else {
												db_entry.issue_project_state =
													None;
											}
											if let Some(matrix_id) =
												github_to_matrix
													.get(&actor.login)
													.and_then(|matrix_id| {
														matrix::parse_id(
															matrix_id,
														)
													}) {
												matrix_bot.send_private_message(&matrix_id, &ISSUE_REVERT_PROJECT_NOTIFICATION.replace("{1}", &issue_html_url))?;
											} else {
												// no matrix id to message
											}
											DbEntryState::Update
										}
									}
								}
							}
							Some(ProjectState::Confirmed(confirmed_id)) => {
								let confirmed_matches_last = db_entry
									.last_confirmed_project
									.map(|proj_id| proj_id == confirmed_id)
									.unwrap_or(false);

								if !confirmed_matches_last {
									db_entry.issue_confirm_project_ping = None;
									db_entry.last_confirmed_project =
										Some(confirmed_id);
								}

								if project_id != &confirmed_id {
									// project has been changed since
									// the confirmation
									db_entry.issue_confirm_project_ping =
										Some(SystemTime::now());
									db_entry.issue_project_state = Some(
										ProjectState::Unconfirmed(*project_id),
									);
									if let Some(room_id) =
										&project_info.matrix_room_id
									{
										matrix_bot.send_public_message(
											&room_id,
											&ISSUE_CONFIRM_PROJECT_MESSAGE
												.replace(
													"{issue_url}",
													issue_html_url,
												)
												.replace(
													"{project_url}",
													project_html_url,
												)
												.replace(
													"{issue_id}",
													&format!("{}", issue_id),
												)
												.replace(
													"{project_id}",
													&format!("{}", project_id),
												),
										)?;
									} else {
										// project info should include matrix
										// room id. TODO some kind of warning
										// here
									}
								}
								DbEntryState::Update
							}
							Some(ProjectState::Denied(_)) => {
								db_entry.issue_confirm_project_ping = None;
								if let Some(prev_project) =
									db_entry.last_confirmed_project
								{
									db_entry.issue_project_state = Some(
										ProjectState::Confirmed(prev_project),
									);
								} else {
									db_entry.issue_project_state = None;
								}
								if let Some(matrix_id) =
									github_to_matrix.get(&actor.login).and_then(
										|matrix_id| matrix::parse_id(matrix_id),
									) {
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
					} else {
						// actor is admin so allow any change
						DbEntryState::DoNothing
					}
				} else {
					// no key in in Projects.toml matches the project name
					unimplemented!()
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
