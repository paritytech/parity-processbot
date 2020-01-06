use rocksdb::{
	IteratorMode,
	DB,
};
use serde::{
	Deserialize,
	Serialize,
};
use std::time::{
	Duration,
	SystemTime,
};

/// Bitflag indicating no action has been taken
pub const NoAction: u32 = 0b00000000;

/// Bitflag indicating an issue has been incorrectly assigned
/// for at least 24h and an appropriate action has been taken
pub const PullRequestCoreDevAuthorIssueNotAssigned24h: u32 = 0b00000010;

/// Bitflag indicating an issue has been incorrectly assigned
/// for at least 72h and an appropriate action has been taken
pub const PullRequestCoreDevAuthorIssueNotAssigned72h: u32 = 0b00000100;

pub enum DbEntryState {
	Delete,
	Update,
	DoNothing,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DbEntry {
	pub actions_taken: u32,
	pub issue_not_assigned_ping: Option<SystemTime>,
	pub issue_no_project_ping: Option<SystemTime>,
	pub issue_no_project_npings: u64,
	pub status_failure_ping: Option<SystemTime>,
}

impl DbEntry {
	pub fn new() -> DbEntry {
		DbEntry {
			actions_taken: NoAction,
			issue_not_assigned_ping: None,
			issue_no_project_ping: None,
			issue_no_project_npings: 0,
			status_failure_ping: None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_bitflags() {
		assert_eq!(
			PullRequestCoreDevAuthorIssueNotAssigned24h
				& PullRequestCoreDevAuthorIssueNotAssigned72h,
			NoAction
		);
		assert_eq!(
			PullRequestCoreDevAuthorIssueNotAssigned24h
				| PullRequestCoreDevAuthorIssueNotAssigned72h,
			0b00000110
		);
		assert_eq!(
			PullRequestCoreDevAuthorIssueNotAssigned24h & NoAction,
			NoAction
		);
		assert_eq!(
			PullRequestCoreDevAuthorIssueNotAssigned24h | NoAction,
			PullRequestCoreDevAuthorIssueNotAssigned24h
		);
	}
}
