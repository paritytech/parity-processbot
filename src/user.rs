use crate::github;

impl github::User {
	pub fn is_assignee(&self, issue: &github::Issue) -> bool {
		issue
			.assignee
			.as_ref()
			.map_or(false, |issue_assignee| issue_assignee.id == self.id)
	}
}
