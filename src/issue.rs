use crate::{
	error, github, github_bot::GithubBot, matrix_bot::MatrixBot, project,
	Result,
};
use itertools::Itertools;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;

const ISSUE_NEEDS_A_PROJECT_MESSAGE: &str =
	"{1} needs to be attached to a project or it will be closed.";

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

pub fn handle_issue(
	_db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: Option<&project::Projects>,
	issue: &github::Issue,
) -> Result<()> {
	// TODO: handle multiple projcets in a single repo
	let project_info =
		projects.and_then(|p| p.0.iter().last().map(|p| p.1.clone()));

	let author = &issue.user;
	let repo = &issue.repository;
	let author_info = project_info
		.map_or_else(project::AuthorInfo::default, |p| {
			p.author_info(&author.login)
		});
	let author_is_core = core_devs.iter().any(|u| u.id == author.id);

	let issue_id = issue.id.context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;

	match issue_actor_and_project(issue, github_bot)? {
		None => {
			if author_info.is_owner || author_info.is_whitelisted {
				// leave a comment and message the author that the issue needs a
				// project
				github_bot.add_comment(
					&repo.name,
					issue_id,
					&ISSUE_NEEDS_A_PROJECT_MESSAGE
						.replace("{1}", &issue_html_url),
				)?;
				if let Some(matrix_id) =
					github_to_matrix.get(&author.login).map(|m| m)
				{
					matrix_bot.send_private_message(
						&matrix_id,
						&ISSUE_NEEDS_A_PROJECT_MESSAGE
							.replace("{1}", &issue_html_url),
					)?;
				}
			} else if author_is_core {
				// ..otherwise if the owner is a core developer, sent a message
				// to "Core Developers" room on Riot with the title of the issue
				// and a link. Send a reminder at 8 hour intervals thereafter.
				// If after 3 days there is still no project attached, move the
				// issue to Core Sorting repository;
			} else {
				// ..otherwise, sent a message to the "Core Developers" room on
				// Riot with the title of the issue and a link. If after 15
				// minutes there is still no project attached, move the issue to
				// Core Sorting repository.
			}
		}
		Some((_actor, _project)) => {}
	}
	Ok(())
}
