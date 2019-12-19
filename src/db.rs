use rocksdb::{IteratorMode, DB};
use serde::{Deserialize, Serialize};

pub const NoAction: u32 = 0b00000000;
pub const PullRequestCoreDevAuthorIssueNotAssigned: u32 = 0b00000001;
pub const PullRequestCoreDevAuthorIssueNotAssigned24h: u32 = 0b00000010;
pub const PullRequestCoreDevAuthorIssueNotAssigned72h: u32 = 0b00000100;
pub const PullRequestStatusFailure: u32 = 0b00001000;

#[derive(Serialize, Deserialize, Debug)]
pub struct DbEntry {
	pub actions_taken: u32,
	pub status_failure_ping_count: u32,
}
