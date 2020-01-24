use crate::db::*;
use crate::local_state::*;
use crate::{
	constants::*, duration_ticks::DurationTicks, error, github,
	github_bot::GithubBot, issue::issue_actor_and_project_card, matrix,
	matrix_bot::MatrixBot, process, Result,
};
use parking_lot::RwLock;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

async fn require_reviewers(
	db: &Arc<RwLock<DB>>,
	pull_request: &github::PullRequest,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	process_info: &process::ProcessInfo,
	reviews: &[github::Review],
	requested_reviewers: &github::RequestedReviewers,
) -> Result<()> {
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;

	log::info!("Requiring reviewers on {}", pr_html_url);

	let reviewer_count = {
		let mut users = reviews
			.iter()
			.map(|r| &r.user)
			.chain(requested_reviewers.users.iter().by_ref())
			.collect::<Vec<&github::User>>();
		users.dedup_by_key(|u| &u.login);
		users.len()
	};

	let owner_or_delegate_requested = reviews
		.iter()
		.map(|r| &r.user)
		.chain(requested_reviewers.users.iter().by_ref())
		.any(|u| process_info.owner_or_delegate() == &u.login);

	let author_info = process_info.author_info(
		&pull_request
			.user
			.as_ref()
			.context(error::MissingData)?
			.login,
	);

	if !author_info.is_owner_or_delegate && !owner_or_delegate_requested {
		log::info!(
			"Requesting a review on {} from the project owner.",
			pr_html_url
		);
		let github_login = process_info.owner_or_delegate();
		if let Some(matrix_id) = github_to_matrix.get(github_login) {
			matrix_bot.send_private_message(
				db,
				&matrix_id,
				&REQUEST_DELEGATED_REVIEW_MESSAGE.replace("{1}", &pr_html_url),
			)?;
			github_bot
				.request_reviews(&pull_request, &[github_login.as_ref()])
				.await?;
		} else {
			log::error!(
                "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
                &github_login
            );
		}
	} else if reviewer_count < MIN_REVIEWERS {
		// post a message in the project's Riot channel, requesting a review;
		// repeat this message every 24 hours until a reviewer is assigned.
		log::info!(
			"Requesting a review on {} from the project room.",
			pr_html_url
		);
		matrix_bot.send_to_room(
			&process_info.matrix_room_id,
			&REQUESTING_REVIEWS_MESSAGE.replace("{1}", &pr_html_url),
		)?;
	} else {
		// do nothing
	}

	Ok(())
}

async fn author_is_core_unassigned(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	process_info: &process::ProcessInfo,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
) -> Result<()> {
	let days = local_state
		.issue_not_assigned_ping()
		.and_then(|ping| ping.elapsed().ok())
		.ticks(ISSUE_NOT_ASSIGNED_PING_PERIOD);
	log::info!("Author of {} is a core developer and the issue has been unassigned for {:?} days.", pull_request.title.as_ref().context(error::MissingData)?, days);
	match days {
		// notify the the issue assignee and project
		// owner through a PM
		None => author_is_core_unassigned_ticks_none(
			db,
			local_state,
			matrix_bot,
			github_to_matrix,
			process_info,
			pull_request,
			issue,
		),
		// do nothing
		Some(0) => Ok(()),
		// if after 24 hours there is no change, then
		// send a message into the project's Riot
		// channel
		Some(1) | Some(2) => author_is_core_unassigned_ticks_passed(
			db,
			local_state,
			matrix_bot,
			process_info,
			pull_request,
			issue,
		),
		// if after a further 48 hours there is still no
		// change, then close the PR.
		_ => {
			author_is_core_unassigned_ticks_expired(
				db,
				local_state,
				github_bot,
				repo,
				pull_request,
			)
			.await
		}
	}
}

fn author_is_core_unassigned_ticks_none(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
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
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	local_state.update_issue_not_assigned_ping(Some(SystemTime::now()), db)?;
	if let Some(assignee) = &issue.assignee {
		if let Some(matrix_id) = github_to_matrix
			.get(&assignee.login)
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
		{
			matrix_bot.send_private_message(
				db,
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
	if let Some(ref matrix_id) = github_to_matrix
		.get(process_info.owner_or_delegate())
		.and_then(|matrix_id| matrix::parse_id(matrix_id))
	{
		matrix_bot.send_private_message(
			db,
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
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	process_info: &process::ProcessInfo,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
) -> Result<()> {
	log::info!("Author of {} is a core developer and the issue is still unassigned to them.", pull_request.title.as_ref().context(error::MissingData)?);
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;
	let issue_html_url = issue.html_url.as_ref().context(error::MissingData)?;
	if local_state.actions_taken() & PullRequestCoreDevAuthorIssueNotAssigned24h
		== NoAction
	{
		local_state.update_actions_taken(
			local_state.actions_taken()
				| PullRequestCoreDevAuthorIssueNotAssigned24h,
			db,
		)?;
		matrix_bot.send_to_room(
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
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
) -> Result<()> {
	log::info!("Author of {} is a core developer and the issue is still unassigned to them, so the PR will be closed.", pull_request.title.as_ref().context(error::MissingData)?);
	if local_state.actions_taken() & PullRequestCoreDevAuthorIssueNotAssigned72h
		== NoAction
	{
		local_state.update_actions_taken(
			local_state.actions_taken()
				| PullRequestCoreDevAuthorIssueNotAssigned72h,
			db,
		)?;
		github_bot
			.close_pull_request(
				&repo.name,
				pull_request.number.context(error::MissingData)?,
			)
			.await?;
	}
	Ok(())
}

async fn handle_status(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	process_info: &process::ProcessInfo,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
	status: &github::Status,
	reviews: &[github::Review],
) -> Result<()> {
	let pr_number = pull_request.number.context(error::MissingData)?;
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;

	let owner_login = process_info.owner_or_delegate();
	let owner_or_delegate_approved = reviews
		.iter()
		.find(|r| &r.user.login == owner_login)
		.map_or(false, |r| r.state.as_deref() == Some("APPROVED"));

	match status.state {
		github::StatusState::Failure => {
			// notify PR author by PM every 24 hours
			let should_ping =
				local_state.status_failure_ping().map_or(true, |ping_time| {
					ping_time.elapsed().ok().map_or(true, |elapsed| {
						elapsed.as_secs() > STATUS_FAILURE_PING_PERIOD
					})
				});

			if should_ping {
				local_state
					.update_status_failure_ping(Some(SystemTime::now()), db)?;
				if let Some(ref matrix_id) = github_to_matrix
					.get(owner_login)
					.and_then(|matrix_id| matrix::parse_id(matrix_id))
				{
					matrix_bot.send_private_message(
						db,
						matrix_id,
						&STATUS_FAILURE_NOTIFICATION
							.replace("{1}", &format!("{}", pr_html_url)),
					)?;
				} else {
					log::error!("Couldn't send a message to the project owner; either their Github or Matrix handle is not set in Bamboo");
				}
			}
		}
		github::StatusState::Success => {
			if owner_or_delegate_approved {
				// merge & delete branch
				github_bot.merge_pull_request(&repo.name, pr_number).await?;
				local_state.delete(db, &local_state.key)?;
			} else {
				local_state.update_status_failure_ping(None, db)?;
			}
		}
		github::StatusState::Pending => {}
	}
	Ok(())
}

async fn handle_pull_request_with_issue_and_project(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	_core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	process_info: &process::ProcessInfo,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
	status: &github::Status,
	reviews: &[github::Review],
	requested_reviewers: &github::RequestedReviewers,
) -> Result<()> {
	let author = pull_request.user.as_ref().context(error::MissingData)?;
	let author_info = process_info.author_info(&author.login);
	let author_is_assignee = issue
		.assignee
		.as_ref()
		.map_or(false, |issue_assignee| issue_assignee.id == author.id);
	if author_is_assignee {
		require_reviewers(
			db,
			&pull_request,
			github_bot,
			matrix_bot,
			github_to_matrix,
			process_info,
			&reviews,
			&requested_reviewers,
		)
		.await?;
	} else {
		if author_info.is_special() {
			// assign the issue to the author
			github_bot
				.assign_issue(&repo.name, issue.number, &author.login)
				.await?;
			require_reviewers(
				db,
				&pull_request,
				github_bot,
				matrix_bot,
				github_to_matrix,
				process_info,
				&reviews,
				&requested_reviewers,
			)
			.await?;
		} else {
			// treat external and core devs the same
			// TODO clarify behaviour
			author_is_core_unassigned(
				db,
				local_state,
				github_bot,
				matrix_bot,
				github_to_matrix,
				process_info,
				repo,
				pull_request,
				&issue,
			)
			.await?;
		}
	}

	handle_status(
		db,
		local_state,
		github_bot,
		matrix_bot,
		github_to_matrix,
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
	db: &Arc<RwLock<DB>>,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	github_login: &str,
	pull_request: &github::PullRequest,
	repo: &github::Repository,
) -> Result<()> {
	let msg = format!("Pull request '{issue_title:?}' in repo '{repo_name}' needs a project attached or it will be closed.",
        issue_title = pull_request.title,
        repo_name = repo.name
    );
	matrix_bot.message_mapped_or_default(
		db,
		github_to_matrix,
		&github_login,
		&msg,
	)
}

async fn author_core_no_project(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	pull_request: &github::PullRequest,
	repo: &github::Repository,
) -> Result<()> {
	let author = pull_request.user.as_ref().context(error::MissingData)?;
	let since = local_state
		.issue_no_project_ping()
		.and_then(|ping| ping.elapsed().ok());
	let ticks = since.ticks(ISSUE_NO_PROJECT_CORE_PING_PERIOD);
	match ticks {
		None => {
			// send a message to the author
			local_state
				.update_issue_no_project_ping(Some(SystemTime::now()), db)?;
			send_needs_project_message(
				db,
				matrix_bot,
				github_to_matrix,
				&author.login,
				pull_request,
				repo,
			)?;
		}
		Some(0) => {}
		Some(i) => {
			if i >= ISSUE_NO_PROJECT_ACTION_AFTER_NPINGS {
				// If after 3 days there is still no project
				// attached, close the pr
				github_bot
					.close_pull_request(
						&repo.name,
						pull_request.number.context(error::MissingData)?,
					)
					.await?;
				local_state.delete(db, &local_state.key)?;
			} else {
				local_state.update_issue_no_project_npings(i, db)?;
				matrix_bot.send_to_default(
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

async fn author_unknown_no_project(
	db: &Arc<RwLock<DB>>,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	pull_request: &github::PullRequest,
	repo: &github::Repository,
) -> Result<()> {
	let author = pull_request.user.as_ref().context(error::MissingData)?;
	let since = local_state
		.issue_no_project_ping()
		.and_then(|ping| ping.elapsed().ok());

	let ticks = since.ticks(ISSUE_NO_PROJECT_NON_CORE_PING_PERIOD);
	match ticks {
		None => {
			// send a message to the author
			local_state
				.update_issue_no_project_ping(Some(SystemTime::now()), db)?;
			send_needs_project_message(
				db,
				matrix_bot,
				github_to_matrix,
				&author.login,
				pull_request,
				repo,
			)?;
		}
		Some(0) => {}
		Some(_) => {
			// If after 15 minutes there is still no project
			// attached, close the pull request
			github_bot
				.close_pull_request(
					&repo.name,
					pull_request.number.context(error::MissingData)?,
				)
				.await?;
			local_state.delete(db, &local_state.key)?;
		}
	}
	Ok(())
}

pub async fn handle_pull_request(
	db: &Arc<RwLock<DB>>,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: &[(Option<github::Project>, process::ProcessInfo)],
	repo: &github::Repository,
	pull_request: &github::PullRequest,
) -> Result<()> {
	let pr_id = pull_request.id.context(error::MissingData)?;
	let pr_number = pull_request.number.context(error::MissingData)?;
	let db_key = format!("{}", pr_id).into_bytes();
	let mut local_state = LocalState::get_or_default(db, db_key)?;

	let author = pull_request.user.as_ref().context(error::MissingData)?;
	let author_is_core = core_devs.iter().any(|u| u.id == author.id);

	let (reviews, issues, status, requested_reviewers) = futures::try_join!(
		github_bot.reviews(pull_request),
		github_bot.pull_request_issues(repo, pull_request),
		github_bot.status(&repo.name, pull_request),
		github_bot.requested_reviewers(pull_request)
	)?;

	match projects.len() {
		0 => { /* no process info so do nothing */ }

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
			if issues.len() > 0 {
				// TODO consider all mentioned issues here
				let issue = issues.first().unwrap();
				handle_pull_request_with_issue_and_project(
					db,
					&mut local_state,
					github_bot,
					matrix_bot,
					core_devs,
					github_to_matrix,
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
				if author_info.is_special() {
					// owners and whitelisted devs can open prs without an attached issue.
					require_reviewers(
						db,
						&pull_request,
						github_bot,
						matrix_bot,
						github_to_matrix,
						process_info,
						&reviews,
						&requested_reviewers,
					)
					.await?;
					handle_status(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						github_to_matrix,
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
					github_bot
						.create_issue_comment(
							&repo.name,
							pr_number,
							&ISSUE_MUST_EXIST_MESSAGE,
						)
						.await?;
					github_bot
						.close_pull_request(&repo.name, pr_number)
						.await?;
				}
			}
		}

		_ => {
			if issues.len() > 0 {
				// TODO consider all mentioned issues here
				let issue = issues.first().unwrap();
				if let Some((_, card)) = issue_actor_and_project_card(
					&repo.name,
					issue.number,
					github_bot,
				)
				.await?
				.or(issue_actor_and_project_card(
					&repo.name,
					pull_request.number.context(error::MissingData)?,
					github_bot,
				)
				.await?)
				{
					let project: github::Project =
						github_bot.project(&card).await?;

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
						handle_pull_request_with_issue_and_project(
							db,
							&mut local_state,
							github_bot,
							matrix_bot,
							core_devs,
							github_to_matrix,
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
						// notify the author that this pr/issue needs a project attached or it will be
						// closed.
						log::info!(
                            "Pull request '{issue_title:?}' in repo '{repo_name}' addresses an issue attached to a project not listed in Process.toml; ignoring",
                            issue_title = pull_request.title,
                            repo_name = repo.name
                        );
						// TODO clarify behaviour here
					}
				} else {
					// notify the author that this pr/issue needs a project attached or it will be
					// closed.
					log::info!(
                        "Pull request '{issue_title:?}' in repo '{repo_name}' addresses an issue unattached to any project",
                        issue_title = pull_request.title,
                        repo_name = repo.name
                    );

					if author_is_core
						|| projects
							.iter()
							.find(|(_, p)| {
								issue.user.as_ref().map_or(false, |user| {
									p.is_special(&user.login)
								})
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
							github_to_matrix,
							pull_request,
							repo,
						)
						.await?;
					} else {
						author_unknown_no_project(
							db,
							&mut local_state,
							github_bot,
							matrix_bot,
							github_to_matrix,
							pull_request,
							repo,
						)
						.await?;
					}
				}
			} else {
				if projects.iter().any(|(_, p)| p.is_special(&author.login)) {
					// author is special so notify them that the pr needs an issue and project
					// attached or it will be closed.
					author_core_no_project(
						db,
						&mut local_state,
						github_bot,
						matrix_bot,
						github_to_matrix,
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
					github_bot
						.create_issue_comment(
							&repo.name,
							pr_number,
							&ISSUE_MUST_BE_VALID_MESSAGE,
						)
						.await?;
					github_bot
						.close_pull_request(&repo.name, pr_number)
						.await?;
				}
			}
		}
	}

	Ok(())
}
