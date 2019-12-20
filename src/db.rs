use rocksdb::{IteratorMode, DB};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

pub const NoAction: u32 = 0b00000000;
pub const PullRequestCoreDevAuthorIssueNotAssigned24h: u32 = 0b00000010;
pub const PullRequestCoreDevAuthorIssueNotAssigned72h: u32 = 0b00000100;

#[derive(Serialize, Deserialize, Debug)]
pub struct DbEntry {
	pub actions_taken: u32,
        pub issue_not_assigned_ping: Option<SystemTime>,
	pub status_failure_ping: Option<SystemTime>,
}
