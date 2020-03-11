use futures_util::future::FutureExt;
use snafu::OptionExt;

use crate::{
	bots, constants::*, duration_ticks::DurationTicks, error, github,
	local_state::LocalState, process, Result,
};

impl bots::Bot {
	pub async fn move_to_backlog(
		&self,
		issue: &dyn github::GithubIssue,
		process_info: &process::ProcessInfo,
		project: &github::Project,
	) -> Result<()> {
		// get the project's backlog column or use the default
		if let Some(backlog_column) = self
			.github_bot
			.project_column_by_name(
				project,
				process_info
					.backlog
					.as_ref()
					.unwrap_or(&BACKLOG_DEFAULT_NAME.to_owned()),
			)
			.await?
		{
			log::info!(
				"Attaching {issue_url} to project '{project_name}'",
				issue_url = issue.html_url(),
				project_name = project.name,
			);
			self.github_bot
				.create_project_card(
					backlog_column.id,
					issue.id(),
					github::ProjectCardContentType::Issue,
				)
				.await?;
		} else {
			self.matrix_bot.send_to_room(
				&process_info.matrix_room_id,
				&PROJECT_NEEDS_BACKLOG
					.replace("{owner}", process_info.owner_or_delegate())
					.replace(
						"{project_url}",
						project
							.html_url
							.as_ref()
							.context(error::MissingData)?,
					),
			)?;
		}
		Ok(())
	}

	pub async fn try_attach_project(
		&self,
		local_state: &mut LocalState,
		issue: &dyn github::GithubIssue,
		projects: &[github::Project],
		process: &[process::ProcessInfo],
		author_is_core: bool,
	) -> Result<()> {
		assert!(!projects.is_empty());
		if projects.len() == 1 {
			let project = projects.first().unwrap();
			let process_info = process
				.iter()
				.find(|proc| proc.project_name == project.name)
				.expect("only one project and process must match at least one");
			self.move_to_backlog(issue, &process_info, &project).await?;
		} else {
			let special_of = process
				.iter()
				.filter(|proc| proc.is_special(&issue.user().login))
				.collect::<Vec<&process::ProcessInfo>>();
			if special_of.len() == 1 {
				let process_info = special_of.first().unwrap();
				let project = projects
					.iter()
					.find(|proj| proj.name == process_info.project_name)
					.expect("every process entry must match a project");
				self.move_to_backlog(issue, &process_info, &project).await?;
			} else {
				// cannot confidently attach project
				log::info!(
					"'{issue_url}' is not attached to any project.",
					issue_url = issue.html_url(),
				);
				self.send_project_notification(
					local_state,
					issue,
					author_is_core,
				)
				.await?;
			}
		}

		Ok(())
	}

	pub async fn issue_project<'a>(
		&self,
		repo_name: &str,
		issue_number: i64,
		projects: &'a [github::Project],
	) -> Option<&'a github::Project> {
		self.github_bot
			.active_project_event(repo_name, issue_number)
			.map(|result| {
				result
					.ok()
					.and_then(|event| {
						event.map(|event| event.project_card).flatten()
					})
					.and_then(|card| {
						projects.iter().find(|proj| card.project_id == proj.id)
					})
			})
			.await
	}

	async fn send_project_notification(
		&self,
		local_state: &mut LocalState,
		issue: &dyn github::GithubIssue,
		author_is_core: bool,
	) -> Result<()> {
		let (ping_time, close_ticks) = if author_is_core {
			(
				self.config.no_project_author_is_core_ping,
				self.config.no_project_author_is_core_close_pr
					/ self.config.no_project_author_is_core_ping,
			)
		} else {
			(self.config.no_project_author_unknown_close_pr, 1)
		};

		let since = local_state
			.issue_no_project_ping()
			.and_then(|ping| ping.elapsed().ok());
		let ticks = since.ticks(ping_time);
		match ticks {
			None => {
				local_state.update_issue_no_project_ping(
					Some(std::time::SystemTime::now()),
					&self.db,
				)?;
				self.matrix_bot.send_to_default(
					&WILL_CLOSE_FOR_NO_PROJECT
						.replace("{author}", &issue.user().login)
						.replace("{issue_url}", issue.html_url()),
				)?;
			}
			Some(0) => {}
			Some(i) => {
				if i >= close_ticks {
					// If after some timeout there is still no project
					// attached, move the issue to Core Sorting
					// repository
					self.github_bot
						.close_issue(
							&issue
								.repository()
								.context(error::MissingData)?
								.name,
							issue.number(),
						)
						.await?;
					self.github_bot
						.create_issue(
							&self.config.core_sorting_repo_name,
							issue.title().unwrap_or(&"".to_owned()),
							issue.body().unwrap_or(&"".to_owned()),
							issue
								.assignee()
								.map(|a| a.login.as_ref())
								.unwrap_or(&"".to_owned()),
						)
						.await?;
					local_state.alive = false;
				} else {
					local_state.update_issue_no_project_npings(i, &self.db)?;
					self.matrix_bot.send_to_default(
						&WILL_CLOSE_FOR_NO_PROJECT
							.replace("{author}", &issue.user().login)
							.replace("{issue_url}", issue.html_url()),
					)?;
				}
			}
		}
		Ok(())
	}
}
