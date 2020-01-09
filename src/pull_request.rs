use crate::db::*;
use crate::{
	constants::*, error, github, github_bot::GithubBot, matrix,
	matrix_bot::MatrixBot, project_info, Result,
};
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::time::SystemTime;

/*
 * if they are not the Delegated Reviewer (by default the project owner),
 * then Require a Review from the Delegated Reviewer; otherwise, if the
 * author is not the project owner, then Require a Review from the
 * Project Owner; otherwise, post a message in the project's Riot
 * channel, requesting a review; repeat this message every 24 hours until
 * a reviewer is assigned.
 */
fn require_reviewer(
	pull_request: &github::PullRequest,
	repo: &github::Repository,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	github_to_matrix: &HashMap<String, String>,
	project_info: Option<&project_info::ProjectInfo>,
) -> Result<()> {
	let author_info = project_info
		.map_or_else(project_info::AuthorInfo::default, |p| {
			p.author_info(&pull_request.user.login)
		});
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;
	let pr_number = pull_request.number.context(error::MissingData)?;

	if !author_info.is_delegated_reviewer {
		if let Some((github_id, matrix_id)) = project_info
			.and_then(|p| p.delegated_reviewer.as_ref())
			.and_then(|u| github_to_matrix.get(u).map(|m| (u, m)))
		{
			matrix_bot.send_private_message(
				&matrix_id,
				&REQUEST_DELEGATED_REVIEW_MESSAGE.replace("{1}", &pr_html_url),
			)?;
			github_bot.request_reviews(
				&repo.name,
				pr_number,
				&[github_id.as_ref()],
			)?;
		}
	} else if !author_info.is_owner {
		if let Some((github_id, matrix_id)) = project_info
			.and_then(|p| p.owner.as_ref())
			.and_then(|u| github_to_matrix.get(u).map(|m| (u, m)))
		{
			matrix_bot.send_private_message(
				&matrix_id,
				&REQUEST_OWNER_REVIEW_MESSAGE.replace("{1}", &pr_html_url),
			)?;
			github_bot.request_reviews(
				&repo.name,
				pr_number,
				&[github_id.as_ref()],
			)?;
		}
	} else {
		// post a message in the project's Riot channel, requesting a review;
		// repeat this message every 24 hours until a reviewer is assigned.
		if let Some(ref room_id) =
			&project_info.and_then(|p| p.matrix_room_id.as_ref())
		{
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
	}

	Ok(())
}

pub fn handle_pull_request(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
	projects: Option<&Vec<(github::Project, project_info::ProjectInfo)>>,
	pull_request: &github::PullRequest,
) -> Result<()> {
	// TODO: handle multiple projcets in a single repo
	let project_info = projects.and_then(|p| p.last().map(|p| p.1.clone()));

	let pr_id = pull_request.id.context(error::MissingData)?;
	let pr_number = pull_request.number.context(error::MissingData)?;
	let db_key = format!("{}", pr_id).into_bytes();
	let mut local_state = LocalState::get_or_new(db, db_key)?;

	let author = &pull_request.user;
	let author_info = project_info
		.as_ref()
		.map_or_else(project_info::AuthorInfo::default, |p| {
			p.author_info(&author.login)
		});
	let repo = pull_request
		.repository
		.as_ref()
		.context(error::MissingData)?;
	let pr_html_url =
		pull_request.html_url.as_ref().context(error::MissingData)?;

	let reviews = github_bot.reviews(pull_request)?;
	let issue = github_bot.issue(pull_request)?;
	let mut statuses = github_bot.statuses(pull_request)?;

	let author_is_core = core_devs.iter().any(|u| u.id == author.id);

	if !(author_info.is_owner || author_info.is_whitelisted) {
		match issue {
			Some(github::Issue {
				id: issue_id,
				html_url: ref issue_html_url,
				assignee: ref issue_assignee,
				..
			}) => {
				let author_is_assignee =
					issue_assignee.as_ref().map_or(false, |issue_assignee| {
						issue_assignee.id == author.id
					});
				if author_is_assignee {
					require_reviewer(
						&pull_request,
						&repo,
						github_bot,
						matrix_bot,
						github_to_matrix,
						project_info.as_ref(),
					)?;
				} else {
					let issue_id = issue_id.context(error::MissingData)?;
					let issue_html_url =
						issue_html_url.as_ref().context(error::MissingData)?;
					if author_info.is_owner || author_info.is_whitelisted {
						// never true ... ?
						// assign the issue to the author
						github_bot.assign_author(
							&repo.name,
							issue_id,
							&author.login,
						)?;
						require_reviewer(
							&pull_request,
							&repo,
							github_bot,
							matrix_bot,
							github_to_matrix,
							project_info.as_ref(),
						)?;
					} else if author_is_core {
						let days = local_state
							.issue_not_assigned_ping()
							.and_then(|ping| ping.elapsed().ok())
							.map(|elapsed| {
								elapsed.as_secs()
									/ ISSUE_NOT_ASSIGNED_PING_PERIOD
							});
						match days {
							None => {
								// notify the the issue assignee and project
								// owner through a PM
								local_state.update_issue_not_assigned_ping(
									Some(SystemTime::now()),
									db,
								)?;
								if let Some(assignee) = issue_assignee {
									if let Some(matrix_id) = github_to_matrix
										.get(&assignee.login)
										.and_then(|matrix_id| {
											matrix::parse_id(matrix_id)
										}) {
										matrix_bot.send_private_message(
											&matrix_id,
											&ISSUE_ASSIGNEE_NOTIFICATION
												.replace("{1}", &pr_html_url)
												.replace("{2}", &issue_html_url)
												.replace("{3}", &author.login),
										)?;
									} else {
										log::warn!(
											"Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
											&assignee.login
										);
									}
								}
								if let Some(matrix_id) = github_to_matrix
									.get(&repo.owner.login)
									.and_then(|matrix_id| {
										matrix::parse_id(matrix_id)
									}) {
									matrix_bot.send_private_message(
										&matrix_id,
										&ISSUE_ASSIGNEE_NOTIFICATION
											.replace("{1}", &pr_html_url)
											.replace("{2}", &issue_html_url)
											.replace("{3}", &author.login),
									)?;
								} else {
									log::warn!(
										"Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
										&repo.owner.login
									);
								}
							}
							Some(0) => { /* do nothing */ }
							Some(1) | Some(2) => {
								// if after 24 hours there is no change, then
								// send a message into the project's Riot
								// channel
								if local_state.actions_taken() & PullRequestCoreDevAuthorIssueNotAssigned24h == NoAction {
									local_state.update_actions_taken(local_state.actions_taken() | PullRequestCoreDevAuthorIssueNotAssigned24h, db)?;
									if let Some(ref room_id) = project_info.and_then(|p| p.matrix_room_id) {
										matrix_bot.send_public_message(
											&room_id,
											&ISSUE_ASSIGNEE_NOTIFICATION
												.replace("{1}", &pr_html_url)
												.replace("{2}", &issue_html_url)
												.replace("{3}", &author.login),
										)?;
									} else {
										matrix_bot.send_public_message(
											&FALLBACK_ROOM_ID,
											&ISSUE_ASSIGNEE_NOTIFICATION
												.replace("{1}", &pr_html_url)
												.replace("{2}", &issue_html_url)
												.replace("{3}", &author.login),
										)?;
									}
								}
							}
							_ => {
								// if after a further 48 hours there is still no
								// change, then close the PR.
								if local_state.actions_taken() & PullRequestCoreDevAuthorIssueNotAssigned72h == NoAction {
									local_state.update_actions_taken(local_state.actions_taken() | PullRequestCoreDevAuthorIssueNotAssigned72h, db)?;
									github_bot.close_pull_request(&repo.name, pr_number)?;
								}
							}
						}
					}
				}
			}
			None => {
				// leave a message that a corresponding issue must exist for
				// each PR close the PR
				github_bot.add_comment(
					&repo.name,
					pr_id,
					&ISSUE_MUST_EXIST_MESSAGE,
				)?;
				github_bot.close_pull_request(&repo.name, pr_number)?;
			}
		}
	}

	let owner_approved = repo.project_owner().map_or(false, |owner| {
		reviews
			.iter()
			.find(|r| r.user.id == owner.id)
			.map_or(false, |r| r.state.as_deref() == Some("APPROVED"))
	});
	let status = statuses.as_mut().and_then(|v| {
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
					if let Some(matrix_id) = github_to_matrix
						.get(&repo.owner.login)
						.and_then(|matrix_id| matrix::parse_id(matrix_id))
					{
						matrix_bot.send_private_message(
							&matrix_id,
							&STATUS_FAILURE_NOTIFICATION
								.replace("{1}", &format!("{}", pr_html_url)),
						)?;
					} else {
						log::warn!(
							"Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
							&repo.owner.login
						);
					}
				}
			}
			github::StatusState::Success => {
				if owner_approved {
					// merge & delete branch
					github_bot.merge_pull_request(&repo.name, pr_number)?;
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
