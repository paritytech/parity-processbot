use crate::db::*;
use crate::{error, github, github_bot::GithubBot, Result};
use rocksdb::DB;
use snafu::ResultExt;

fn require_reviewer(author: &github::User, repo: &github::Repository) {
	let author_is_owner = repo
		.project_owner()
		.map_or(false, |owner| owner.id == author.id);
	let author_is_delegated = repo.delegated_reviewer().as_ref().unwrap_or(&repo.owner).id == author.id;

	if !author_is_delegated {
		// require review from delegated reviewer
	} else if !author_is_owner {
		// require review from project owner
	} else {
		// post a message in the project's Riot channel, requesting a review; repeat this message every 24 hours until a reviewer is assigned.
	}

	/*
	 * if they are not the Delegated Reviewer (by default the project owner), then Require a Review from the Delegated Reviewer;
	 * otherwise, if the author is not the project owner, then Require a Review from the Project Owner;
	 * otherwise, post a message in the project's Riot channel, requesting a review; repeat this message every 24 hours until a reviewer is assigned.
	 */
}

pub fn handle_pull_request(
	db: &DB,
	bot: &GithubBot,
	pull_request: &github::PullRequest,
) -> Result<()> {
	let pr_id = pull_request.id;
        let db_key = &format!("{}", pr_id).into_bytes();
	let mut db_entry = DbEntry {
		actions_taken: NoAction,
		status_failure_ping_count: 0,
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
	let reviews = bot.reviews(pull_request)?;
	let issue = bot.issue(pull_request)?;
	let statuses = bot.statuses(pull_request)?;

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
					.map_or(false, |issue_assignee| issue_assignee.id == author.id)
				{
					require_reviewer(&author, &repo);
				} else {
					if author_is_owner || author_is_whitelisted {
						// never true ... ?
						// assign the issue to the author
						require_reviewer(&author, &repo);
					} else if author.is_core_developer() {
						if db_entry.actions_taken & PullRequestCoreDevAuthorIssueNotAssigned
							== NoAction
						{
							// notify the the issue assignee and project owner through a PM
							db_entry.actions_taken |= PullRequestCoreDevAuthorIssueNotAssigned;
						} else if db_entry.actions_taken
							& PullRequestCoreDevAuthorIssueNotAssigned24h
							== NoAction
						{
							// if after 24 hours there is no change, then send a message into the project's Riot channel
							db_entry.actions_taken |= PullRequestCoreDevAuthorIssueNotAssigned24h;
						} else if db_entry.actions_taken
							& PullRequestCoreDevAuthorIssueNotAssigned72h
							== NoAction
						{
							// if after a further 48 hours there is still no change, then close the PR.
							db_entry.actions_taken |= PullRequestCoreDevAuthorIssueNotAssigned72h;
						}
					} else {
						// do nothing
					}
				}
			}
			None => {
				// leave a message that a corresponding issue must exist for each PR
				// close the PR
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
			if db_entry.actions_taken & PullRequestStatusFailure == NoAction {
				// notify PR author by PM
				db_entry.actions_taken |= PullRequestStatusFailure;
			} else {
				// as long as it continues, repeat every 24 hours.
			}
		} else if status.state == "success" && owner_approved {
			// merge & delete branch
		}
	} else {
		// pull request has no status
	}

	db.delete(db_key)
		.context(error::Db)?;
	db.put(db_key,
		serde_json::to_string(&db_entry)
			.expect("serialize db entry")
			.as_bytes(),
	)
	.unwrap();
	Ok(())
}
