use futures_util::future::FutureExt;
use snafu::OptionExt;

use crate::{
	bots, constants::*, db::*, duration_ticks::DurationTicks, error, github,
	local_state::LocalState, Result,
};

impl bots::Bot {
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

	pub async fn check_issue_project<'a>(
		&self,
		local_state: &mut LocalState,
		repo: &github::Repository,
		issue: &dyn github::GithubIssue,
		projects: &'a [github::Project],
	) -> Result<Option<&'a github::Project>> {
		let author_is_core =
			self.core_devs.iter().any(|u| issue.user().id == u.id);
		match self
			.issue_project(&repo.name, issue.number(), projects)
			.await
		{
			None => {
				log::debug!(
                    "'{issue_url}' is not attached to any project in repo '{repo_name}'",
                    issue_url = issue.html_url(),
                    repo_name = repo.name
                );
				self.send_project_notification(
					local_state,
					issue,
					author_is_core,
				)
				.await?;
				Ok(None)
			}
			project => Ok(project),
		}
	}
}
