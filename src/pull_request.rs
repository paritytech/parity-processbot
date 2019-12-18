use crate::db::*;
use crate::developer::Developer;
use crate::issue::Issue;
use crate::repository::Repository;
use crate::review::Review;
use rocksdb::DB;

pub trait PullRequest {
	fn pull_request_id(&self) -> String;
	fn issue(&self) -> Option<Box<dyn Issue>>;
	fn author(&self) -> Box<dyn Developer>;
	fn repository(&self) -> Box<dyn Repository>;
        fn status(&self) -> String;
        fn reviews(&self) -> Vec<Box<dyn Review>>;
}

fn require_reviewer(author: Box<dyn Developer>, repo: &dyn Repository) {
	let author_is_owner = repo
		.project_owner()
		.map_or(false, |owner| owner.user_id() == author.user_id());
	let author_is_delegated = repo
		.delegated_reviewer()
		.or(repo.project_owner())
		.map_or(false, |reviewer| reviewer.user_id() == author.user_id());

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

pub fn handle_pull_request(db: &DB, pull_request: &dyn PullRequest) {
	let pr_id = pull_request.pull_request_id();
	let mut db_entry = DbEntry {
		actions_taken: NoAction,
		status_failure_ping_count: 0,
	};
	if let Ok(Some(entry)) = db.get_pinned(pr_id.as_bytes()).map(|v| {
		v.map(|value| {
			serde_json::from_str::<DbEntry>(String::from_utf8(value.to_vec()).unwrap().as_str())
				.expect("deserialize entry")
		})
	}) {
		db_entry = entry;
	}

	let author = pull_request.author();
	let repo = pull_request.repository();

	let author_is_owner = repo
		.project_owner()
		.map_or(false, |owner| owner.user_id() == author.user_id());
	let author_is_whitelisted = repo
		.whitelist()
		.iter()
		.find(|w| w.user_id() == author.user_id())
		.is_some();

	if !(author_is_owner || author_is_whitelisted) {
		match pull_request.issue() {
			Some(issue) => {
				if issue.assignee().map_or(false, |issue_assignee| {
					issue_assignee.user_id() == author.user_id()
				}) {
					require_reviewer(author, &*repo);
				} else {
					if author_is_owner || author_is_whitelisted {
						// never true ... ?
						// assign the issue to the author
						require_reviewer(author, &*repo);
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

	let status = pull_request.status();
	let owner_approved = repo.project_owner().map_or(false, |owner| {
		pull_request
			.reviews()
			.iter()
			.find(|r| r.user().user_id() == owner.user_id()).map_or(false, |r| r.state() == "APPROVED")
	});
	if status == "failure" {
		if db_entry.actions_taken & PullRequestStatusFailure == NoAction {
			// notify PR author by PM
			db_entry.actions_taken |= PullRequestStatusFailure;
		} else {
			// as long as it continues, repeat every 24 hours.
		}
	} else if status == "success" && owner_approved {
		// merge & delete branch
	}

	db.delete(&pr_id.as_bytes()).unwrap();
	db.put(
		pr_id.as_bytes(),
		serde_json::to_string(&db_entry)
			.expect("serialize db entry")
			.as_bytes(),
	)
	.unwrap();
}
