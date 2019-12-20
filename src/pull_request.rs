use crate::db::*;
use crate::{error, github, github_bot::GithubBot, matrix_bot::MatrixBot, Result};
use rocksdb::DB;
use snafu::ResultExt;
use std::time::{Duration, SystemTime};

const STATUS_FAILURE_PING_PERIOD: u64 = 3600 * 24;
const ISSUE_NOT_ASSIGNED_PING_PERIOD: u64 = 3600 * 24;
const FALLBACK_ROOM_ID: &'static str = "!aenJixaHcSKbJOWxYk:matrix.parity.io";
const ISSUE_MUST_EXIST_MESSAGE: &'static str = "Every pull request must address an issue.";
const ISSUE_ASSIGNEE_NOTIFICATION: &'static str = "{1} addressing {2} has been opened by {3}. Please reassign the issue or close the pull request.";
const REQUESTING_REVIEWS_MESSAGE: &'static str = "{1} is in need of reviewers.";
const STATUS_FAILURE_NOTIFICATION: &'static str = "{1} has failed status checks.";

fn require_reviewer(
	pull_request: &github::PullRequest,
	repo: &github::Repository,
	project_info: &github::ProjectInfo,
	matrix_bot: &MatrixBot,
) {
	let author_is_owner = repo
		.project_owner()
		.map_or(false, |owner| owner.id == pull_request.user.id);
	let author_is_delegated =
		repo.delegated_reviewer().as_ref().unwrap_or(&repo.owner).id == pull_request.user.id;

	if !author_is_delegated {
		// require review from delegated reviewer
	} else if !author_is_owner {
		// require review from project owner
	} else {
		// post a message in the project's Riot channel, requesting a review; repeat this message every 24 hours until a reviewer is assigned.
		if let Some(room_id) = &project_info.room_id {
			matrix_bot.send_public_message(
				&room_id,
				&REQUESTING_REVIEWS_MESSAGE.replace("{1}", &format!("{}", pull_request.html_url)),
			);
		} else {
			matrix_bot.send_public_message(
				&FALLBACK_ROOM_ID,
				&REQUESTING_REVIEWS_MESSAGE.replace("{1}", &format!("{}", pull_request.html_url)),
			);
		}
	}

	/*
	 * if they are not the Delegated Reviewer (by default the project owner), then Require a Review from the Delegated Reviewer;
	 * otherwise, if the author is not the project owner, then Require a Review from the Project Owner;
	 * otherwise, post a message in the project's Riot channel, requesting a review; repeat this message every 24 hours until a reviewer is assigned.
	 */
}

pub fn handle_pull_request(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	pull_request: &github::PullRequest,
) -> Result<()> {
	let pr_id = pull_request.id;
	let db_key = &format!("{}", pr_id).into_bytes();
	let mut db_entry = DbEntry {
		actions_taken: NoAction,
		issue_not_assigned_ping: None,
		status_failure_ping: None,
	};
	if let Ok(Some(entry)) = db.get_pinned(db_key).map(|v| {
		v.map(|value| {
			serde_json::from_str::<DbEntry>(String::from_utf8(value.to_vec()).unwrap().as_str())
				.expect("deserialize entry")
		})
	}) {
		db_entry = entry;
	}

	let author = &pull_request.user;
	let repo = &pull_request.repository;
	let reviews = github_bot.reviews(pull_request)?;
	let issue = github_bot.issue(pull_request)?;
	let statuses = github_bot.statuses(pull_request)?;
	let project_info = github_bot.project_info(&pull_request.repository)?;

	let author_is_owner = repo.owner.id == author.id;
	let author_is_whitelisted = repo
		.whitelist()
		.iter()
		.find(|w| w.id == author.id)
		.is_some();

	if !(author_is_owner || author_is_whitelisted) {
		match issue {
			Some(issue) => {
				if issue
					.assignee
					.as_ref()
					.map_or(false, |issue_assignee| issue_assignee.id == author.id)
				{
					require_reviewer(&pull_request, &repo, &project_info, matrix_bot);
				} else {
					if author_is_owner || author_is_whitelisted {
						// never true ... ?
						// assign the issue to the author
						github_bot.assign_author(&repo.name, issue.id, &author.login)?;
						require_reviewer(&pull_request, &repo, &project_info, matrix_bot);
					} else if author.is_core_developer() {
						let days = db_entry
							.issue_not_assigned_ping
							.and_then(|ping| ping.elapsed().ok())
							.map(|elapsed| elapsed.as_secs() / ISSUE_NOT_ASSIGNED_PING_PERIOD);
						match days {
							None => {
								// notify the the issue assignee and project owner through a PM
								db_entry.issue_not_assigned_ping = Some(SystemTime::now());
								if let Some(assignee) = issue.assignee {
									matrix_bot.send_private_message(
										&assignee.riot_id(),
										&ISSUE_ASSIGNEE_NOTIFICATION
											.replace("{1}", &format!("{}", pull_request.html_url))
											.replace("{2}", &format!("{}", issue.html_url))
											.replace("{3}", &format!("{}", author.login)),
									);
								}
								matrix_bot.send_private_message(
									&repo.owner.riot_id(),
									&ISSUE_ASSIGNEE_NOTIFICATION
										.replace("{1}", &format!("{}", pull_request.html_url))
										.replace("{2}", &format!("{}", issue.html_url))
										.replace("{3}", &format!("{}", author.login)),
								);
							}
							Some(0) => { /* do nothing */ }
							Some(1) | Some(2) => {
								// if after 24 hours there is no change, then send a message into the project's Riot channel
								if db_entry.actions_taken
									& PullRequestCoreDevAuthorIssueNotAssigned24h
									== NoAction
								{
									db_entry.actions_taken |=
										PullRequestCoreDevAuthorIssueNotAssigned24h;
									if let Some(room_id) = project_info.room_id {
										matrix_bot.send_public_message(
											&room_id,
											&ISSUE_ASSIGNEE_NOTIFICATION
												.replace(
													"{1}",
													&format!("{}", pull_request.html_url),
												)
												.replace("{2}", &format!("{}", issue.html_url))
												.replace("{3}", &format!("{}", author.login)),
										);
									} else {
										matrix_bot.send_public_message(
											&FALLBACK_ROOM_ID,
											&ISSUE_ASSIGNEE_NOTIFICATION
												.replace(
													"{1}",
													&format!("{}", pull_request.html_url),
												)
												.replace("{2}", &format!("{}", issue.html_url))
												.replace("{3}", &format!("{}", author.login)),
										);
									}
								}
							}
							_ => {
								// if after a further 48 hours there is still no change, then close the PR.
								if db_entry.actions_taken
									& PullRequestCoreDevAuthorIssueNotAssigned72h
									== NoAction
								{
									db_entry.actions_taken |=
										PullRequestCoreDevAuthorIssueNotAssigned72h;
								}
							}
						}
					}
				}
			}
			None => {
				// leave a message that a corresponding issue must exist for each PR
				// close the PR
				github_bot.add_comment(&repo.name, pull_request.id, &ISSUE_MUST_EXIST_MESSAGE);
			}
		}
	}

	let owner_approved = repo.project_owner().map_or(false, |owner| {
		reviews
			.iter()
			.find(|r| r.user.id == owner.id)
			.map_or(false, |r| r.state == "APPROVED")
	});
	if let Some(status) = statuses.first() {
		if status.state == "failure" {
			// notify PR author by PM every 24 hours
			if db_entry.status_failure_ping.map_or(true, |ping_time| {
				ping_time.elapsed().map_or(true, |elapsed| {
					elapsed.as_secs() > STATUS_FAILURE_PING_PERIOD
				})
			}) {
				db_entry.status_failure_ping = Some(SystemTime::now());
				matrix_bot.send_private_message(
					&pull_request.user.riot_id(),
					&STATUS_FAILURE_NOTIFICATION
						.replace("{1}", &format!("{}", pull_request.html_url)),
				);
			}
		} else if status.state == "success" {
			if owner_approved {
				// merge & delete branch
				db.delete(db_key).context(error::Db)?;
				return Ok(());
			} else {
				db_entry.status_failure_ping = None;
			}
		}
	} else {
		// pull request has no status
	}

	db.delete(db_key).context(error::Db)?;
	db.put(
		db_key,
		serde_json::to_string(&db_entry)
			.expect("serialize db entry")
			.as_bytes(),
	)
	.unwrap();
	Ok(())
}
