use crate::{bots, constants::*, github, Result};

impl bots::Bot {
	pub async fn pr_missing_issue(
		&self,
		local_state: &mut LocalState,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		let elapsed = local_state
			.issue_not_addressed_ping()
			.and_then(|ping| ping.elapsed().ok());
		let days = elapsed.ticks(self.config.issue_not_addressed_ping);
		log::info!(
			"{} has been missing an issue for at least {:?} seconds.",
			pull_request.html_url,
			elapsed
		);
		match days {
			// notify the the issue assignee and project
			// owner through a PM
			None => {
				self.github_bot
					.create_issue_comment(
						&repo.name,
						pull_request.number,
						&WARN_FOR_NO_ISSUE
							.replace("{author}", &pull_request.user.login),
					)
					.await?
			}
			// do nothing
			Some(0) => Ok(()),
			// if after 24 hours there is no change, then
			// send a message into the project's Riot
			// channel
			Some(_) => {
                log::info!(
                    "Closing pull request '{issue_title:?}' as it addresses no issue in repo '{repo_name}'",
                    issue_title = pull_request.title,
                    repo_name = repo.name
                );
				self.github_bot
					.create_issue_comment(
						&repo.name,
						pull_request.number,
						&CLOSE_FOR_NO_ISSUE
							.replace("{author}", &pull_request.user.login),
					)
					.await?;
				self.github_bot
					.close_pull_request(&repo.name, pull_request.number)
					.await?;
                local_state.alive = false;
			}
		}
		Ok(())
	}

	/*
	/// Return the project card attached to an issue, if there is one, and the user who attached it
	pub async fn issue_actor_and_project_card(
		&self,
		repo_name: &str,
		issue_number: i64,
	) -> Result<Option<(github::User, github::ProjectCard)>> {
		Ok(self
			.github_bot
			.active_project_event(repo_name, issue_number)
			.await?
			.and_then(|mut issue_event| {
				issue_event
					.project_card
					.take()
					.map(|card| (issue_event.actor, card))
			}))
	}

	fn author_non_special_project_state_none(
		&self,
		local_state: &mut LocalState,
		issue: &github::Issue,
		project: &github::Project,
		project_column: &github::ProjectColumn,
		process_info: &process::ProcessInfo,
		actor: &github::User,
	) -> Result<()> {
		local_state.update_issue_confirm_project_ping(
			Some(SystemTime::now()),
			&self.db,
		)?;
		local_state.update_issue_project(
			Some(IssueProject {
				state: IssueProjectState::Unconfirmed,
				actor_login: actor.login.clone(),
				project_column_id: project_column.id,
			}),
			&self.db,
		)?;
		let matrix_id = self
			.github_to_matrix
			.get(&process_info.owner)
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
			.unwrap_or("the project owner".to_owned());
		self.matrix_bot.send_to_room(
			&process_info.matrix_room_id,
			&PROJECT_CONFIRMATION
				.replace(
					"{issue_url}",
					issue.html_url.as_ref().context(error::MissingData)?,
				)
				.replace(
					"{column_name}",
					project_column.name.as_ref().context(error::MissingData)?,
				)
				.replace("{project_name}", &project.name)
				.replace("{owner}", &matrix_id)
				.replace(
					"{issue_id}",
					&format!("{}", issue.id.context(error::MissingData)?),
				)
				.replace("{column_id}", &format!("{}", project_column.id))
				.replace(
					"{seconds}",
					&format!("{}", self.config.project_confirmation_timeout),
				),
		)?;
		Ok(())
	}

	async fn author_non_special_project_state_unconfirmed(
		&self,
		local_state: &mut LocalState,
		issue: &github::Issue,
		project: &github::Project,
		project_column: &github::ProjectColumn,
		process_info: &process::ProcessInfo,
		actor: &github::User,
	) -> Result<()> {
		let issue_id = issue.id.context(error::MissingData)?;
		let issue_html_url =
			issue.html_url.as_ref().context(error::MissingData)?;

		let issue_project =
			local_state.issue_project().expect("has to be Some here");
		let unconfirmed_id = issue_project.project_column_id;

		if project_column.id != unconfirmed_id {
			local_state.update_issue_confirm_project_ping(
				Some(SystemTime::now()),
				&self.db,
			)?;
			local_state.update_issue_project(
				Some(IssueProject {
					state: IssueProjectState::Unconfirmed,
					actor_login: actor.login.clone(),
					project_column_id: project_column.id,
				}),
				&self.db,
			)?;
			let matrix_id = self
				.github_to_matrix
				.get(&process_info.owner)
				.and_then(|matrix_id| matrix::parse_id(matrix_id))
				.unwrap_or("the project owner".to_owned());
			self.matrix_bot.send_to_room(
				&process_info.matrix_room_id,
				&PROJECT_CONFIRMATION
					.replace(
						"{issue_url}",
						issue.html_url.as_ref().context(error::MissingData)?,
					)
					.replace(
						"{column_name}",
						project_column
							.name
							.as_ref()
							.context(error::MissingData)?,
					)
					.replace("{project_name}", &project.name)
					.replace("{owner}", &matrix_id)
					.replace(
						"{issue_id}",
						&format!("{}", issue.id.context(error::MissingData)?),
					)
					.replace("{column_id}", &format!("{}", project_column.id))
					.replace(
						"{seconds}",
						&format!(
							"{}",
							self.config.project_confirmation_timeout
						),
					),
			)?;
		} else {
			let ticks = local_state
				.issue_confirm_project_ping()
				.and_then(|t| t.elapsed().ok())
				.ticks(self.config.project_confirmation_timeout);

			match ticks.expect("don't know how long to wait for confirmation; shouldn't ever allow issue_project_state to be set without updating issue_confirm_project_ping") {
			0 => {}
			_ => {
				// confirmation timeout. delete project card and reattach last
				// confirmed if possible
				local_state.update_issue_confirm_project_ping(None, &self.db)?;
				local_state.update_issue_project(
					local_state.last_confirmed_issue_project().cloned(),
					&self.db,
				)?;
				self.github_bot.delete_project_card(unconfirmed_id).await?;
				if let Some(prev_column_id) =
					local_state.issue_project().map(|p| p.project_column_id)
				{
					// reattach the last confirmed project
					self.github_bot.create_project_card(
						prev_column_id,
						issue_id,
						github::ProjectCardContentType::Issue,
					).await?;
				}
				if let Some(matrix_id) = self.github_to_matrix
					.get(&actor.login)
					.and_then(|matrix_id| matrix::parse_id(matrix_id))
				{
					self.matrix_bot.send_private_message(
						&self.db,
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
		&self,
		local_state: &mut LocalState,
		issue: &github::Issue,
		project: &github::Project,
		project_column: &github::ProjectColumn,
		process_info: &process::ProcessInfo,
		actor: &github::User,
	) -> Result<()> {
		let issue_id = issue.id.context(error::MissingData)?;
		let issue_html_url =
			issue.html_url.as_ref().context(error::MissingData)?;
		let denied_id = local_state.issue_project().unwrap().project_column_id;

		if project_column.id != denied_id {
			local_state.update_issue_confirm_project_ping(
				Some(SystemTime::now()),
				&self.db,
			)?;
			local_state.update_issue_project(
				Some(IssueProject {
					state: IssueProjectState::Unconfirmed,
					actor_login: actor.login.clone(),
					project_column_id: project_column.id,
				}),
				&self.db,
			)?;
			let matrix_id = self
				.github_to_matrix
				.get(&process_info.owner)
				.and_then(|matrix_id| matrix::parse_id(matrix_id))
				.unwrap_or("the project owner".to_owned());
			self.matrix_bot.send_to_room(
				&process_info.matrix_room_id,
				&PROJECT_CONFIRMATION
					.replace(
						"{issue_url}",
						issue.html_url.as_ref().context(error::MissingData)?,
					)
					.replace(
						"{column_name}",
						project_column
							.name
							.as_ref()
							.context(error::MissingData)?,
					)
					.replace("{project_name}", &project.name)
					.replace("{owner}", &matrix_id)
					.replace(
						"{issue_id}",
						&format!("{}", issue.id.context(error::MissingData)?),
					)
					.replace("{column_id}", &format!("{}", project_column.id))
					.replace(
						"{seconds}",
						&format!(
							"{}",
							self.config.project_confirmation_timeout
						),
					),
			)?;
		} else {
			local_state.update_issue_confirm_project_ping(None, &self.db)?;
			local_state.update_issue_project(
				local_state.last_confirmed_issue_project().cloned(),
				&self.db,
			)?;
			self.github_bot.delete_project_card(denied_id).await?;
			if let Some(prev_column_id) =
				local_state.issue_project().map(|p| p.project_column_id)
			{
				// reattach the last confirmed project
				self.github_bot
					.create_project_card(
						prev_column_id,
						issue_id,
						github::ProjectCardContentType::Issue,
					)
					.await?;
			}
		}
		if let Some(matrix_id) = self
			.github_to_matrix
			.get(&local_state.issue_project().unwrap().actor_login)
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
		{
			self.matrix_bot.send_private_message(
				&self.db,
				&matrix_id,
				&ISSUE_REVERT_PROJECT_NOTIFICATION
					.replace("{issue_url}", &issue_html_url),
			)?;
		}
		Ok(())
	}

	fn author_non_special_project_state_confirmed(
		&self,
		local_state: &mut LocalState,
		issue: &github::Issue,
		project: &github::Project,
		project_column: &github::ProjectColumn,
		process_info: &process::ProcessInfo,
		actor: &github::User,
	) -> Result<()> {
		let confirmed_id =
			local_state.issue_project().unwrap().project_column_id;

		let confirmed_matches_last = local_state
			.last_confirmed_issue_project()
			.map(|proj| proj.project_column_id == confirmed_id)
			.unwrap_or(false);

		if !confirmed_matches_last {
			local_state.update_issue_confirm_project_ping(None, &self.db)?;
			local_state.update_last_confirmed_issue_project(
				local_state.issue_project().cloned(),
				&self.db,
			)?;
		}

		if project_column.id != confirmed_id {
			// project has been changed since
			// the confirmation
			local_state.update_issue_confirm_project_ping(
				Some(SystemTime::now()),
				&self.db,
			)?;
			local_state.update_issue_project(
				Some(IssueProject {
					state: IssueProjectState::Unconfirmed,
					actor_login: actor.login.clone(),
					project_column_id: project_column.id,
				}),
				&self.db,
			)?;
			let matrix_id = self
				.github_to_matrix
				.get(&process_info.owner)
				.and_then(|matrix_id| matrix::parse_id(matrix_id))
				.unwrap_or("the project owner".to_owned());
			self.matrix_bot.send_to_room(
				&process_info.matrix_room_id,
				&PROJECT_CONFIRMATION
					.replace(
						"{issue_url}",
						issue.html_url.as_ref().context(error::MissingData)?,
					)
					.replace(
						"{column_name}",
						project_column
							.name
							.as_ref()
							.context(error::MissingData)?,
					)
					.replace("{project_name}", &project.name)
					.replace("{owner}", &matrix_id)
					.replace(
						"{issue_id}",
						&format!("{}", issue.id.context(error::MissingData)?),
					)
					.replace("{column_id}", &format!("{}", project_column.id))
					.replace(
						"{seconds}",
						&format!(
							"{}",
							self.config.project_confirmation_timeout
						),
					),
			)?;
		}
		Ok(())
	}

	pub async fn handle_issue(
		&self,
		combined_process: &process::CombinedProcessInfo,
		repo: &github::Repository,
		issue: &github::Issue,
		projects: &[github::Project],
	) -> Result<()> {
		let issue_id = issue.id.context(error::MissingData)?;

		let db_key = issue_id.to_le_bytes().to_vec();
		let mut local_state = LocalState::get_or_default(&self.db, db_key)?;

		let author_is_core =
			self.core_devs.iter().any(|u| issue.user.id == u.id);

		if combined_process.len() == 0 {
			// there are no projects matching those listed in Process.toml so do nothing
		} else {
			match self
				.issue_actor_and_project_card(&repo.name, issue.number)
				.await?
			{
				None => {
					if self.feature_config.issue_project_valid {
						log::debug!(
							"Handling issue '{issue_title}' with no project in repo '{repo_name}'",
							issue_title = issue.title.as_ref().unwrap_or(&"".to_owned()),
							repo_name = repo.name
						);

						let since = local_state
							.issue_no_project_ping()
							.and_then(|ping| ping.elapsed().ok());
						let special_of_project =
							combined_process.get(&issue.user.login);

						if combined_process.len() == 1
							&& special_of_project.is_some()
						{
							// repo contains only one project and the author is special
							// so we can attach it with high confidence
							self.author_special_attach_only_project(
								&mut local_state,
								issue,
								special_of_project.expect("checked above"),
								projects.iter().find(|x| x.name == special_of_project.unwrap().project_name).expect("entries in combined_process all match a project"),
							)
							.await?;
						} else if author_is_core
							|| combined_process.is_special(&issue.user.login)
						{
							// author is a core developer or special of at least one
							// project in the repo
							self.issue_author_core_no_project(
								&mut local_state,
								issue,
								since,
							)
							.await?;
						} else {
							// author is neither core developer nor special
							self.issue_author_unknown_no_project(
								&mut local_state,
								issue,
								since,
							)
							.await?;
						}
					}
				}
				Some((_actor, _card)) => {
					/*
						if self.feature_config.issue_project_changes {
							let project: github::Project =
								self.github_bot.project(&card).await?;
							let project_column: github::ProjectColumn = self
								.github_bot
								.project_column_by_name(
									&project,
									card.column_name
										.as_ref()
										.context(error::MissingData)?,
								)
								.await?
								.context(error::MissingData)?;
							//							self.github_bot.project_column(&card).await?;

							log::debug!(
								"Handling issue '{issue_title}' in project '{project_name}' in repo '{repo_name}'",
								issue_title = issue.title.as_ref().unwrap_or(&"".to_owned()),
								project_name = project.name,
								repo_name = repo.name
							);

							if let Some(process_info) = projects
								.iter()
								.find(|(p, _)| {
									p.as_ref()
										.map_or(false, |p| &project.name == &p.name)
								})
								.map(|(_, p)| p)
							{
								if !process_info.is_special(&actor.login) {
									// TODO check if confirmation has confirmed/denied.
									// requires parsing messages in project room

									match local_state.issue_project().map(|p| p.state) {
									None => self
										.author_non_special_project_state_none(
											&mut local_state,
											issue,
											&project,
											&project_column,
											&process_info,
											&actor,
										)?,
									Some(IssueProjectState::Unconfirmed) => self
										.author_non_special_project_state_unconfirmed(
											&mut local_state,
											issue,
											&project,
											&project_column,
											&process_info,
											&actor,
										)
										.await?,
									Some(IssueProjectState::Denied) => self
										.author_non_special_project_state_denied(
											&mut local_state,
											issue,
											&project,
											&project_column,
											&process_info,
											&actor,
										)
										.await?,
									Some(IssueProjectState::Confirmed) => self
										.author_non_special_project_state_confirmed(
											&mut local_state,
											issue,
											&project,
											&project_column,
											&process_info,
											&actor,
										)?,
								};
								} else {
									// actor is special so allow any change
								}
							} else {
								// no key in in Process.toml matches the project name
								// TODO notification here
							}
						}
					*/
				}
			}
		}
		Ok(())
	}
		*/
}
