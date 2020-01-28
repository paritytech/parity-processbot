use crate::db::*;
use crate::local_state::*;
use crate::{
	bots, constants::*, duration_ticks::DurationTicks, error, github, matrix,
	process, Result,
};
use snafu::OptionExt;
use std::time::SystemTime;

impl bots::Bot {
	async fn author_is_core_unassigned(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
	) -> Result<()> {
		let days = local_state
			.issue_not_assigned_ping()
			.and_then(|ping| ping.elapsed().ok())
			.ticks(self.config.issue_not_assigned_to_pr_author_ping);
		log::info!(
			"The issue addressed by {} has been unassigned for {:?} days.",
			pull_request.title.as_ref().context(error::MissingData)?,
			days
		);
		match days {
			// notify the the issue assignee and project
			// owner through a PM
			None => self.author_is_core_unassigned_ticks_none(
				local_state,
				process_info,
				pull_request,
				issue,
			),
			// do nothing
			Some(0) => Ok(()),
			// if after 24 hours there is no change, then
			// send a message into the project's Riot
			// channel
			Some(1) | Some(2) => self.author_is_core_unassigned_ticks_passed(
				local_state,
				process_info,
				pull_request,
				issue,
			),
			// if after a further 48 hours there is still no
			// change, then close the PR.
			_ => {
				self.author_is_core_unassigned_ticks_expired(
					local_state,
					repo,
					pull_request,
				)
				.await
			}
		}
	}

	fn author_is_core_unassigned_ticks_none(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
	) -> Result<()> {
		log::info!(
		"Author of {} is a core developer but the issue is unassigned to them.",
		pull_request.title.as_ref().context(error::MissingData)?
	);
		let pr_html_url =
			pull_request.html_url.as_ref().context(error::MissingData)?;
		let issue_html_url =
			issue.html_url.as_ref().context(error::MissingData)?;
		local_state.update_issue_not_assigned_ping(
			Some(SystemTime::now()),
			&self.db,
		)?;
		if let Some(assignee) = &issue.assignee {
			if let Some(matrix_id) = self
				.github_to_matrix
				.get(&assignee.login)
				.and_then(|matrix_id| matrix::parse_id(matrix_id))
			{
				self.matrix_bot.send_private_message(
					&self.db,
					&matrix_id,
					&ISSUE_ASSIGNEE_NOTIFICATION
						.replace("{1}", &pr_html_url)
						.replace("{2}", &issue_html_url)
						.replace(
							"{3}",
							&pull_request
								.user
								.as_ref()
								.context(error::MissingData)?
								.login,
						),
				)?;
			} else {
				log::error!(
                "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
                &assignee.login
            );
			}
		}
		if let Some(ref matrix_id) = self
			.github_to_matrix
			.get(process_info.owner_or_delegate())
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
		{
			self.matrix_bot.send_private_message(
				&self.db,
				matrix_id,
				&ISSUE_ASSIGNEE_NOTIFICATION
					.replace("{1}", &pr_html_url)
					.replace("{2}", &issue_html_url)
					.replace(
						"{3}",
						&pull_request
							.user
							.as_ref()
							.context(error::MissingData)?
							.login,
					),
			)?;
		} else {
			log::error!(
            "Couldn't send a message to the project owner; either their Github or Matrix handle is not set in Bamboo"
        );
		}
		Ok(())
	}

	fn author_is_core_unassigned_ticks_passed(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
	) -> Result<()> {
		log::info!("Author of {} is a core developer and the issue is still unassigned to them.", pull_request.title.as_ref().context(error::MissingData)?);
		let pr_html_url =
			pull_request.html_url.as_ref().context(error::MissingData)?;
		let issue_html_url =
			issue.html_url.as_ref().context(error::MissingData)?;
		if local_state.actions_taken()
			& PullRequestCoreDevAuthorIssueNotAssigned24h
			== NoAction
		{
			local_state.update_actions_taken(
				local_state.actions_taken()
					| PullRequestCoreDevAuthorIssueNotAssigned24h,
				&self.db,
			)?;
			self.matrix_bot.send_to_room(
				&process_info.matrix_room_id,
				&ISSUE_ASSIGNEE_NOTIFICATION
					.replace("{1}", &pr_html_url)
					.replace("{2}", &issue_html_url)
					.replace(
						"{3}",
						&pull_request
							.user
							.as_ref()
							.context(error::MissingData)?
							.login,
					),
			)?;
		}
		Ok(())
	}

	async fn author_is_core_unassigned_ticks_expired(
		&self,
		local_state: &mut LocalState,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		log::info!("Author of {} is a core developer and the issue is still unassigned to them, so the PR will be closed.", pull_request.title.as_ref().context(error::MissingData)?);
		if local_state.actions_taken()
			& PullRequestCoreDevAuthorIssueNotAssigned72h
			== NoAction
		{
			local_state.update_actions_taken(
				local_state.actions_taken()
					| PullRequestCoreDevAuthorIssueNotAssigned72h,
				&self.db,
			)?;
			self.github_bot
				.close_pull_request(
					&repo.name,
					pull_request.number.context(error::MissingData)?,
				)
				.await?;
		}
		Ok(())
	}

	async fn handle_pull_request_with_issue_and_project(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
		status: &github::Status,
		reviews: &[github::Review],
		requested_reviewers: &github::RequestedReviewers,
	) -> Result<()> {
		let author = pull_request.user.as_ref().context(error::MissingData)?;
		if author.is_assignee(issue) {
			self.require_reviewers(
				local_state,
				&pull_request,
				process_info,
				&reviews,
				&requested_reviewers,
			)
			.await?;
		} else {
			if process_info.is_special(&author.login) {
				// assign the issue to the author
				self.github_bot
					.assign_issue(&repo.name, issue.number, &author.login)
					.await?;
				self.require_reviewers(
					local_state,
					&pull_request,
					process_info,
					&reviews,
					&requested_reviewers,
				)
				.await?;
			} else {
				// treat external and core devs the same
				// TODO clarify behaviour
				self.author_is_core_unassigned(
					local_state,
					process_info,
					repo,
					pull_request,
					&issue,
				)
				.await?;
			}
		}

		self.handle_status(
			local_state,
			&process_info,
			&repo,
			&pull_request,
			status,
			&reviews,
		)
		.await?;

		Ok(())
	}

	fn send_needs_project_message(
		&self,
		github_login: &str,
		pull_request: &github::PullRequest,
		repo: &github::Repository,
	) -> Result<()> {
		let msg = format!("Pull request '{issue_title:?}' in repo '{repo_name}' needs a project attached or it will be closed.",
        issue_title = pull_request.title,
        repo_name = repo.name
    );
		self.matrix_bot.message_mapped_or_default(
			&self.db,
			&self.github_to_matrix,
			&github_login,
			&msg,
		)
	}

	async fn pr_author_core_no_project(
		&self,
		local_state: &mut LocalState,
		pull_request: &github::PullRequest,
		repo: &github::Repository,
	) -> Result<()> {
		let author = pull_request.user.as_ref().context(error::MissingData)?;
		let since = local_state
			.issue_no_project_ping()
			.and_then(|ping| ping.elapsed().ok());
		let ticks = since.ticks(self.config.no_project_author_is_core_ping);
		match ticks {
			None => {
				// send a message to the author
				local_state.update_issue_no_project_ping(
					Some(SystemTime::now()),
					&self.db,
				)?;
				self.send_needs_project_message(
					&author.login,
					pull_request,
					repo,
				)?;
			}
			Some(0) => {}
			Some(i) => {
				if i >= self.config.no_project_close_pr
					/ self.config.no_project_author_is_core_ping
				{
					// If after 3 days there is still no project
					// attached, close the pr
					self.github_bot
						.close_pull_request(
							&repo.name,
							pull_request.number.context(error::MissingData)?,
						)
						.await?;
					local_state.delete(&self.db, &local_state.key)?;
				} else {
					local_state.update_issue_no_project_npings(i, &self.db)?;
					self.matrix_bot.send_to_default(
						&ISSUE_NO_PROJECT_MESSAGE.replace(
							"{1}",
							&pull_request
								.html_url
								.as_ref()
								.context(error::MissingData)?,
						),
					)?;
				}
			}
		}
		Ok(())
	}

	async fn pr_author_unknown_no_project(
		&self,
		local_state: &mut LocalState,
		pull_request: &github::PullRequest,
		repo: &github::Repository,
	) -> Result<()> {
		let author = pull_request.user.as_ref().context(error::MissingData)?;
		let since = local_state
			.issue_no_project_ping()
			.and_then(|ping| ping.elapsed().ok());

		let ticks = since.ticks(self.config.no_project_author_not_core_ping);
		match ticks {
			None => {
				// send a message to the author
				local_state.update_issue_no_project_ping(
					Some(SystemTime::now()),
					&self.db,
				)?;
				self.send_needs_project_message(
					&author.login,
					pull_request,
					repo,
				)?;
			}
			Some(0) => {}
			Some(_) => {
				// If after 15 minutes there is still no project
				// attached, close the pull request
				self.github_bot
					.close_pull_request(
						&repo.name,
						pull_request.number.context(error::MissingData)?,
					)
					.await?;
				local_state.delete(&self.db, &local_state.key)?;
			}
		}
		Ok(())
	}

	pub async fn handle_pull_request(
		&self,
		projects: &[(Option<github::Project>, process::ProcessInfo)],
		repo: &github::Repository,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		let pr_id = pull_request.id.context(error::MissingData)?;
		let pr_number = pull_request.number.context(error::MissingData)?;
		let db_key = format!("{}", pr_id).into_bytes();
		let mut local_state = LocalState::get_or_default(&self.db, db_key)?;

		let author = pull_request.user.as_ref().context(error::MissingData)?;
		let author_is_core = self.core_devs.iter().any(|u| u.id == author.id);

		let (reviews, issues, status, requested_reviewers) = futures::try_join!(
			self.github_bot.reviews(pull_request),
			self.github_bot.pull_request_issues(repo, pull_request),
			self.github_bot.status(&repo.name, pull_request),
			self.github_bot.requested_reviewers(pull_request)
		)?;

		match projects.len() {
		0 => log::warn!("should never try to handle pull request without projects / process_info"),

		1 => {
			// assume the sole project is the relevant one
			let (project, process_info) = projects.last().unwrap();
			log::info!(
                "Handling pull request '{issue_title:?}' in project '{project_name:?}' in repo '{repo_name}'",
                issue_title = pull_request.title,
                project_name = project.as_ref().map(|p| &p.name),
                repo_name = repo.name
            );

			let author_info = process_info.author_info(&author.login);

			if issues.len() == 0 {
				if author_info.is_special() {
					// owners and whitelisted devs can open prs without an attached issue.
					self.require_reviewers(
						&mut local_state,
						&pull_request,
						process_info,
						&reviews,
						&requested_reviewers,
					)
					.await?;
					self.handle_status(
						&mut local_state,
						&process_info,
						&repo,
						&pull_request,
						&status,
						&reviews,
					)
					.await?;
				} else {
					// leave a message that a corresponding issue must exist for
					// each PR close the PR
					log::info!(
                        "Closing pull request '{issue_title:?}' as it addresses no issue in repo '{repo_name}'",
                        issue_title = pull_request.title,
                        repo_name = repo.name
                    );
					self.github_bot
						.create_issue_comment(
							&repo.name,
							pr_number,
							&ISSUE_MUST_EXIST_MESSAGE,
						)
						.await?;
					self.github_bot
						.close_pull_request(&repo.name, pr_number)
						.await?;
				}
			} else {
				// TODO consider all mentioned issues here
				let issue = issues.first().unwrap();
				self.handle_pull_request_with_issue_and_project(
					&mut local_state,
					process_info,
					repo,
					pull_request,
					&issue,
					&status,
					&reviews,
					&requested_reviewers,
				)
				.await?;
			}
		}

        // 1+ projects
		_ => {
			if issues.len() == 0 {
				if projects.iter().any(|(_, p)| p.is_special(&author.login)) {
					// author is special so notify them that the pr needs an issue and project
					// attached or it will be closed.
					self.pr_author_core_no_project(
						&mut local_state,
						pull_request,
						repo,
					)
					.await?;
				} else {
					// the pr does not address an issue and the author is not special, so close it.
					log::info!(
                        "Closing pull request '{issue_title:?}' as it addresses no issue in repo '{repo_name}'",
                        issue_title = pull_request.title,
                        repo_name = repo.name
                    );
					self.github_bot
						.create_issue_comment(
							&repo.name,
							pr_number,
							&ISSUE_MUST_BE_VALID_MESSAGE,
						)
						.await?;
					self.github_bot
						.close_pull_request(&repo.name, pr_number)
						.await?;
				}
			} else {
				// TODO consider all mentioned issues here
				let issue = issues.first().unwrap();
				if let Some((_, card)) = self.issue_actor_and_project_card(
					&repo.name,
					issue.number,
				)
				.await?
				.or(self.issue_actor_and_project_card(
					&repo.name,
					pull_request.number.context(error::MissingData)?,
				)
				.await?)
				{
					let project: github::Project =
						self.github_bot.project(&card).await?;

					log::info!(
                        "Handling pull request '{issue_title:?}' in project '{project_name}' in repo '{repo_name}'",
                        issue_title = pull_request.title,
                        project_name = project.name,
                        repo_name = repo.name
                    );

					if let Some(process_info) = projects
						.iter()
						.find(|(p, _)| {
							p.as_ref()
								.map_or(false, |p| &p.name == &project.name)
						})
						.map(|(_, p)| p)
					{
						self.handle_pull_request_with_issue_and_project(
							&mut local_state,
							process_info,
							repo,
							pull_request,
							&issue,
							&status,
							&reviews,
							&requested_reviewers,
						)
						.await?;
					} else {
                        // pull request addresses issue but project not listed in Process.toml
						// TODO clarify behaviour here
						log::info!(
                            "Pull request '{issue_title:?}' in repo '{repo_name}' addresses an issue attached to a project not listed in Process.toml; ignoring",
                            issue_title = pull_request.title,
                            repo_name = repo.name
                        );
					}
				} else {
					// notify the author that this pr/issue needs a project attached or it will be
					// closed.
					log::info!(
                        "Pull request '{issue_title:?}' in repo '{repo_name}' addresses an issue unattached to any project",
                        issue_title = pull_request.title,
                        repo_name = repo.name
                    );

					let author_is_special = projects
						.iter()
						.find(|(_, p)| {
							issue
								.user
								.as_ref()
								.map_or(false, |user| p.is_special(&user.login))
						})
						.is_some();

					if author_is_core || author_is_special {
						// author is a core developer or special of at least one
						// project in the repo
						self.pr_author_core_no_project(
							&mut local_state,
							pull_request,
							repo,
						)
						.await?;
					} else {
						self.pr_author_unknown_no_project(
							&mut local_state,
							pull_request,
							repo,
						)
						.await?;
					}
				}
			}
		}
	}

		Ok(())
	}
}
