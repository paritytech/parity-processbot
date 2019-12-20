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

#[derive(Serialize, Deserialize, Debug)]
pub struct DbEntry {
	pub actions_taken: u32,
	pub issue_not_assigned_ping: Option<SystemTime>,
	pub status_failure_ping: Option<SystemTime>,
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
