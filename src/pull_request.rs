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
		let elapsed = local_state
			.issue_not_assigned_ping()
			.and_then(|ping| ping.elapsed().ok());
		let days =
			elapsed.ticks(self.config.issue_not_assigned_to_pr_author_ping);
		log::info!(
            "Issue {} addressed by {} has been unassigned for at least {:?} seconds.",
            issue.html_url,
            pull_request.title.as_ref().context(error::MissingData)?,
            elapsed
        );
		match days {
			// notify the the issue assignee and project
			// owner through a PM
			None => self.send_private_issue_reassignment_notification(
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
			Some(1) | Some(2) => self
				.send_public_issue_reassignment_notification(
					local_state,
					process_info,
					pull_request,
					issue,
				),
			// if after a further 48 hours there is still no
			// change, then close the PR.
			_ => {
				self.close_for_issue_unassigned(local_state, repo, pull_request)
					.await
			}
		}
	}

	fn send_private_issue_reassignment_notification(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
	) -> Result<()> {
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
					&PRIVATE_ISSUE_NEEDS_REASSIGNMENT
						.replace("{pr_url}", &pull_request.html_url)
						.replace("{issue_url}", &issue.html_url)
						.replace("{author}", &pull_request.user.login),
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
				&PRIVATE_ISSUE_NEEDS_REASSIGNMENT
					.replace("{pr_url}", &pull_request.html_url)
					.replace("{issue_url}", &issue.html_url)
					.replace("{author}", &pull_request.user.login),
			)?;
		} else {
			log::error!(
                "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
                &process_info.owner_or_delegate()
            );
		}
		Ok(())
	}

	fn send_public_issue_reassignment_notification(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
	) -> Result<()> {
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
				&PUBLIC_ISSUE_NEEDS_REASSIGNMENT
					.replace("{owner}", &process_info.owner_or_delegate())
					.replace("{pr_url}", &pull_request.html_url)
					.replace("{issue_url}", &issue.html_url)
					.replace("{author}", &pull_request.user.login),
			)?;
		}
		Ok(())
	}

	async fn close_for_issue_unassigned(
		&self,
		local_state: &mut LocalState,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		log::info!(
			"Closing {}",
			pull_request.title.as_ref().context(error::MissingData)?
		);
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
				.close_pull_request(&repo.name, pull_request.number)
				.await?;
			local_state.alive = false;
		}
		Ok(())
	}

	pub async fn assign_issue_or_warn(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
		issue: &github::Issue,
	) -> Result<()> {
		if process_info.is_special(&pull_request.user.login) {
			// assign the issue to the author
			self.github_bot
				.assign_issue(
					&repo.name,
					issue.number,
					&pull_request.user.login,
				)
				.await?;
		} else {
			if !(issue as &dyn github::GithubIssue)
				.is_assignee(&pull_request.user.login)
			{
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
		if self.feature_config.pr_project_valid {
			let author = &pull_request.user;
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
					if i >= self.config.no_project_author_is_core_close_pr
						/ self.config.no_project_author_is_core_ping
					{
						// If after some timeout there is still no project
						// attached, close the pr
						self.github_bot
							.close_pull_request(&repo.name, pull_request.number)
							.await?;
						local_state.alive = false;
					} else {
						local_state
							.update_issue_no_project_npings(i, &self.db)?;
						self.matrix_bot.send_to_default(
							&WILL_CLOSE_FOR_NO_PROJECT
								.replace("{author}", &pull_request.user.login)
								.replace("{issue_url}", &pull_request.html_url),
						)?;
					}
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
		if self.feature_config.pr_project_valid {
			let author = &pull_request.user;
			let since = local_state
				.issue_no_project_ping()
				.and_then(|ping| ping.elapsed().ok());

			let ticks =
				since.ticks(self.config.no_project_author_unknown_close_pr);
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
					// If after some timeout there is still no project
					// attached, close the pull request
					self.github_bot
						.close_pull_request(&repo.name, pull_request.number)
						.await?;
					local_state.alive = false;
				}
			}
		}
		Ok(())
	}

	pub async fn handle_pull_request(
		&self,
		local_state: &mut LocalState,
		combined_process: &process::CombinedProcessInfo,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
		status: &github::CombinedStatus,
		issues: &[github::Issue],
		requested_reviewers: &github::RequestedReviewers,
		reviews: &[github::Review],
	) -> Result<()> {
		let author = &pull_request.user;
		let author_is_core = self.core_devs.iter().any(|u| u.id == author.id);

		match combined_process.len() {
			0 => {
				// pull request is not attached to any project, nor are its linked
				// issues.
				// notify the author that this pr/issue needs a project attached
				// or it will be closed.
				log::info!(
                    "Pull request '{issue_title:?}' in repo '{repo_name}' is not attached to a project",
                    issue_title = pull_request.title,
                    repo_name = repo.name
                );

				if author_is_core {
					self.pr_author_core_no_project(
						local_state,
						pull_request,
						repo,
					)
					.await?;
				} else {
					self.pr_author_unknown_no_project(
						local_state,
						pull_request,
						repo,
					)
					.await?;
				}
			}

			1 => {
				// assume the sole project is the relevant one
				let process_info = combined_process.iter().last().unwrap();

				log::info!(
                    "Handling pull request '{issue_title:?}' in project '{project_name:?}' in repo '{repo_name}'",
                    issue_title = pull_request.title,
                    project_name = process_info.project_name,
                    repo_name = repo.name
                );

				if issues.len() == 0 {
					if combined_process.is_special(&author.login) {
						// owners and whitelisted devs can open prs without an attached issue.
						self.require_reviewers(
							local_state,
							&pull_request,
							&process_info,
							&reviews,
							&requested_reviewers,
						)
						.await?;
						self.handle_status(local_state, &pull_request, &status)
							.await?;
					} else {
						if self.feature_config.pr_issue_mention {
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
									pull_request.number,
									&CLOSE_FOR_NO_ISSUE
										.replace("{author}", &author.login),
								)
								.await?;
							self.github_bot
								.close_pull_request(
									&repo.name,
									pull_request.number,
								)
								.await?;
							local_state.alive = false;
						}
					}
				} else {
					/*
					for issue in issues {
						self.assign_issue_or_warn(
							local_state,
							process_info,
							repo,
							pull_request,
							&issue,
						)
						.await?;
					}
					*/
					self.require_reviewers(
						local_state,
						&pull_request,
						process_info,
						&reviews,
						&requested_reviewers,
					)
					.await?;
					self.handle_status(local_state, &pull_request, status)
						.await?;
				}
			}

			// 1+ projects
			_ => {
				if let Some(project) = if let Some(card) = self
					.github_bot
					.active_project_event(&repo.name, pull_request.number)
					.await?
					.and_then(|event| event.project_card)
				{
					Some(self.github_bot.project(&card).await?)
				} else {
					None
				} {
					if let Some(process_info) =
						combined_process.get(&project.name)
					{
						if issues.len() == 0 {
							if process_info.is_special(&author.login) {
								// owners and whitelisted devs can open prs without an attached issue.
								self.require_reviewers(
									local_state,
									&pull_request,
									&process_info,
									&reviews,
									&requested_reviewers,
								)
								.await?;
								self.handle_status(
									local_state,
									&pull_request,
									&status,
								)
								.await?;
							} else {
								if self.feature_config.pr_issue_mention {
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
											pull_request.number,
											&CLOSE_FOR_NO_ISSUE.replace(
												"{author}",
												&author.login,
											),
										)
										.await?;
									self.github_bot
										.close_pull_request(
											&repo.name,
											pull_request.number,
										)
										.await?;
									local_state.alive = false;
								}
							}
						} else {
							/*
							for issue in issues {
								self.assign_issue_or_warn(
									local_state,
									&process_info,
									repo,
									pull_request,
									&issue,
								)
								.await?;
							}
							*/
							self.require_reviewers(
								local_state,
								&pull_request,
								&process_info,
								&reviews,
								&requested_reviewers,
							)
							.await?;
							self.handle_status(
								local_state,
								&pull_request,
								status,
							)
							.await?;
						}
					} else {
						// pull request addresses issue but project not listed
						// in Process.toml
						// TODO clarify behaviour here
						log::info!(
                            "Ignoring pull request '{issue_title:?}' in repo '{repo_name}' as it is attached to a project not listed in Process.toml",
                            issue_title = pull_request.title,
                            repo_name = repo.name
                        );
					}
				} else {
					// notify the author that this pr/issue needs a project attached
					// or it will be closed.
					log::info!(
                        "Pull request '{issue_title:?}' in repo '{repo_name}' is not attached to a project",
                        issue_title = pull_request.title,
                        repo_name = repo.name
                    );

					if author_is_core {
						self.pr_author_core_no_project(
							local_state,
							pull_request,
							repo,
						)
						.await?;
					} else {
						self.pr_author_unknown_no_project(
							local_state,
							pull_request,
							repo,
						)
						.await?;
					}
				}
			}
		}

		Ok(())
	}
}
