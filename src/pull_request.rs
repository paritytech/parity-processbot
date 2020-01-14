use crate::db::*;
use crate::{
	constants::*, duration_ticks::DurationTicks, error, github,
	github_bot::GithubBot, issue::issue_actor_and_project_card, matrix,
	matrix_bot::MatrixBot, project_info, Result,
};
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::time::SystemTime;

async fn require_reviewers(
	pull_request: &github::PullRequest,
	repo: &github::Repository,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	project_info: &project_info::ProjectInfo,
	reviews: &[github::Review],
	requested_reviewers: &github::RequestedReviewers,
) -> Result<()> {
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
		.any(|u| {
			project_info
				.owner_or_delegate()
				.map_or(false, |x| &u.login == x)
		});

	let author_info = project_info.author_info(&pull_request.user.login);
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;
	let pr_number = pull_request.number.context(error::MissingData)?;

	if !author_info.is_owner_or_delegate && !owner_or_delegate_requested {
		if let Some((github_id, matrix_id)) = project_info
			.owner_or_delegate()
			.and_then(|u| github_to_matrix.get(u).map(|m| (u, m)))
		{
			matrix_bot.send_private_message(
				&matrix_id,
				&REQUEST_DELEGATED_REVIEW_MESSAGE.replace("{1}", &pr_html_url),
			)?;
			github_bot
				.request_reviews(&repo.name, pr_number, &[github_id.as_ref()])
				.await?;
		}
	} else if reviewer_count < MIN_REVIEWERS {
		// post a message in the project's Riot channel, requesting a review;
		// repeat this message every 24 hours until a reviewer is assigned.
		if let Some(ref room_id) = &project_info.matrix_room_id.as_ref() {
			matrix_bot.send_public_message(
				&room_id,
				&REQUESTING_REVIEWS_MESSAGE.replace("{1}", &pr_html_url),
			)?;
		} else {
			matrix_bot.send_public_message(
				&FALLBACK_ROOM_ID,
				&REQUESTING_REVIEWS_MESSAGE.replace("{1}", &pr_html_url),
			)?;
		}
	} else {
		// do nothing
	}

	Ok(())
}

async fn author_is_core_unassigned(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	project_info: &project_info::ProjectInfo,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
) -> Result<()> {
	let days = local_state
		.issue_not_assigned_ping()
		.and_then(|ping| ping.elapsed().ok())
		.ticks(ISSUE_NOT_ASSIGNED_PING_PERIOD);
	match days {
		// notify the the issue assignee and project
		// owner through a PM
		None => author_is_core_unassigned_ticks_none(
			db,
			local_state,
			matrix_bot,
			github_to_matrix,
			project_info,
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
			project_info,
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
	db: &DB,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	project_info: &project_info::ProjectInfo,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
) -> Result<()> {
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
				&matrix_id,
				&ISSUE_ASSIGNEE_NOTIFICATION
					.replace("{1}", &pr_html_url)
					.replace("{2}", &issue_html_url)
					.replace("{3}", &pull_request.user.login),
			)?;
		} else {
			log::warn!(
                                "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
                                &assignee.login
                        );
		}
	}
	if let Some(ref matrix_id) =
		project_info.owner_or_delegate().and_then(|owner_login| {
			github_to_matrix
				.get(owner_login)
				.and_then(|matrix_id| matrix::parse_id(matrix_id))
		}) {
		matrix_bot.send_private_message(
			matrix_id,
			&ISSUE_ASSIGNEE_NOTIFICATION
				.replace("{1}", &pr_html_url)
				.replace("{2}", &issue_html_url)
				.replace("{3}", &pull_request.user.login),
		)?;
	} else {
		log::warn!(
                        "Couldn't send a message to the project owner; either their Github or Matrix handle is not set in Bamboo"
                );
	}
	Ok(())
}

fn author_is_core_unassigned_ticks_passed(
	db: &DB,
	local_state: &mut LocalState,
	matrix_bot: &MatrixBot,
	project_info: &project_info::ProjectInfo,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
) -> Result<()> {
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
		if let Some(ref room_id) = project_info.matrix_room_id.as_ref() {
			matrix_bot.send_public_message(
				&room_id,
				&ISSUE_ASSIGNEE_NOTIFICATION
					.replace("{1}", &pr_html_url)
					.replace("{2}", &issue_html_url)
					.replace("{3}", &pull_request.user.login),
			)?;
		} else {
			matrix_bot.send_public_message(
				&FALLBACK_ROOM_ID,
				&ISSUE_ASSIGNEE_NOTIFICATION
					.replace("{1}", &pr_html_url)
					.replace("{2}", &issue_html_url)
					.replace("{3}", &pull_request.user.login),
			)?;
		}
	}
	Ok(())
}

async fn author_is_core_unassigned_ticks_expired(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
) -> Result<()> {
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
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	project_info: &project_info::ProjectInfo,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
	statuses: Option<&mut Vec<github::Status>>,
	reviews: &[github::Review],
) -> Result<()> {
	let pr_number = pull_request.number.context(error::MissingData)?;
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;

	let owner_or_delegate_approved =
		project_info
			.owner_or_delegate()
			.map_or(false, |owner_login| {
				reviews
					.iter()
					.find(|r| &r.user.login == owner_login)
					.map_or(false, |r| r.state.as_deref() == Some("APPROVED"))
			});
	let status = statuses.and_then(|v| {
		v.sort_by_key(|s| s.updated_at);
		v.last()
	});

	if let Some(ref status) = status {
		match status.state {
			github::StatusState::Failure => {
				// notify PR author by PM every 24 hours
				let should_ping = local_state.status_failure_ping().map_or(
					true,
					|ping_time| {
						ping_time.elapsed().ok().map_or(true, |elapsed| {
							elapsed.as_secs() > STATUS_FAILURE_PING_PERIOD
						})
					},
				);

				if should_ping {
					local_state.update_status_failure_ping(
						Some(SystemTime::now()),
						db,
					)?;
					if let Some(ref matrix_id) = project_info
						.owner_or_delegate()
						.and_then(|owner_login| {
							github_to_matrix.get(owner_login)
						})
						.and_then(|matrix_id| matrix::parse_id(matrix_id))
					{
						matrix_bot.send_private_message(
							matrix_id,
							&STATUS_FAILURE_NOTIFICATION
								.replace("{1}", &format!("{}", pr_html_url)),
						)?;
					} else {
						log::warn!("Couldn't send a message to the project owner; either their Github or Matrix handle is not set in Bamboo");
					}
				}
			}
			github::StatusState::Success => {
				if owner_or_delegate_approved {
					// merge & delete branch
					github_bot
						.merge_pull_request(&repo.name, pr_number)
						.await?;
					local_state.delete(db)?;
				} else {
					local_state.update_status_failure_ping(None, db)?;
				}
			}
			github::StatusState::Pending => {}
		}
	} else {
		// pull request has no status
		// should never happen
		unimplemented!()
	}
	Ok(())
}

async fn handle_pull_request_with_issue_and_project(
	db: &DB,
	local_state: &mut LocalState,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	project_info: &project_info::ProjectInfo,
	repo: &github::Repository,
	pull_request: &github::PullRequest,
	issue: &github::Issue,
	statuses: Option<&mut Vec<github::Status>>,
	reviews: &[github::Review],
	requested_reviewers: &github::RequestedReviewers,
) -> Result<()> {
	let author = &pull_request.user;
	let author_info = project_info.author_info(&author.login);
	let author_is_core = core_devs.iter().any(|u| u.id == author.id);
	let author_is_assignee = issue
		.assignee
		.as_ref()
		.map_or(false, |issue_assignee| issue_assignee.id == author.id);
	if author_is_assignee {
		require_reviewers(
			&pull_request,
			&repo,
			github_bot,
			matrix_bot,
			github_to_matrix,
			project_info,
			&reviews,
			&requested_reviewers,
		)
		.await?;
	} else {
		let issue_id = issue.id.context(error::MissingData)?;
		if author_info.is_special() {
			// assign the issue to the author
			github_bot
				.assign_author(&repo.name, issue_id, &author.login)
				.await?;
			require_reviewers(
				&pull_request,
				&repo,
				github_bot,
				matrix_bot,
				github_to_matrix,
				project_info,
				&reviews,
				&requested_reviewers,
			)
			.await?;
		} else if author_is_core {
			author_is_core_unassigned(
				db,
				local_state,
				github_bot,
				matrix_bot,
				github_to_matrix,
				project_info,
				repo,
				pull_request,
				&issue,
			)
			.await?;
		} else {
			// do nothing..?
			// TODO clarify behaviour
		}
	}

	handle_status(
		db,
		local_state,
		github_bot,
		matrix_bot,
		github_to_matrix,
		&project_info,
		&repo,
		&pull_request,
		statuses,
		&reviews,
	)
	.await?;
	Ok(())
}

pub async fn handle_pull_request(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: &[(github::Project, project_info::ProjectInfo)],
	pull_request: &github::PullRequest,
) -> Result<()> {
	let (_reviews, requested_reviewers) = futures::try_join!(
		github_bot.reviews(pull_request),
		github_bot.requested_reviewers(pull_request)
	)?;

	let pr_id = pull_request.id.context(error::MissingData)?;
	let pr_number = pull_request.number.context(error::MissingData)?;
	let db_key = format!("{}", pr_id).into_bytes();
	let mut local_state = LocalState::get_or_new(db, db_key)?;

	let author = &pull_request.user;
	let repo = pull_request
		.repository
		.as_ref()
		.context(error::MissingData)?;

	let (reviews, issue, mut statuses) = futures::try_join!(
		github_bot.reviews(pull_request),
		github_bot.issue(pull_request),
		github_bot.statuses(pull_request)
	)?;

	match projects.len() {
		0 => { /* no process info so do nothing */ }
		1 => {
			let (_project, project_info) = projects.last().unwrap();
			let author_info = project_info.author_info(&author.login);
			if let Some(issue) = issue {
				handle_pull_request_with_issue_and_project(
					db,
					&mut local_state,
					github_bot,
					matrix_bot,
					core_devs,
					github_to_matrix,
					project_info,
					repo,
					pull_request,
					&issue,
					statuses.as_mut(),
					&reviews,
					&requested_reviewers,
				)
				.await?;
			} else {
				if author_info.is_special() {
					require_reviewers(
						&pull_request,
						&repo,
						github_bot,
						matrix_bot,
						github_to_matrix,
						project_info,
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
						&project_info,
						&repo,
						&pull_request,
						statuses.as_mut(),
						&reviews,
					)
					.await?;
				} else {
					// leave a message that a corresponding issue must exist for
					// each PR close the PR
					github_bot
						.add_comment(
							&repo.name,
							pr_id,
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
			if let Some(issue) = issue {
				if let Some((_, card)) =
					issue_actor_and_project_card(&issue, github_bot).await?
				{
					let project: github::Project =
						github_bot.project(&card).await?;

					if let Some(project_info) = projects
						.iter()
						.find(|(p, _)| &p.name == &project.name)
						.map(|(_, p)| p)
					{
						handle_pull_request_with_issue_and_project(
							db,
							&mut local_state,
							github_bot,
							matrix_bot,
							core_devs,
							github_to_matrix,
							project_info,
							repo,
							pull_request,
							&issue,
							statuses.as_mut(),
							&reviews,
							&requested_reviewers,
						)
						.await?;
					} else {
						github_bot
							.add_comment(
								&repo.name,
								pr_id,
								&ISSUE_MUST_BE_VALID_MESSAGE,
							)
							.await?;
						github_bot
							.close_pull_request(&repo.name, pr_number)
							.await?;
					}
				} else {
					github_bot
						.add_comment(
							&repo.name,
							pr_id,
							&ISSUE_MUST_BE_VALID_MESSAGE,
						)
						.await?;
					github_bot
						.close_pull_request(&repo.name, pr_number)
						.await?;
				}
			} else {
				github_bot
					.add_comment(
						&repo.name,
						pr_id,
						&ISSUE_MUST_BE_VALID_MESSAGE,
					)
					.await?;
				github_bot.close_pull_request(&repo.name, pr_number).await?;
			}
		}
	}

	Ok(())
}
