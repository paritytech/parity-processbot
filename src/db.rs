// TODO: Move bitflags to use `bitflags` crate.
#![allow(non_upper_case_globals)]
use crate::{error, Result};
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::time::SystemTime;

/// Bitflag indicating no action has been taken
pub const NoAction: u32 = 0b00000000;

/// Bitflag indicating an issue has been incorrectly assigned
/// for at least 24h and an appropriate action has been taken
pub const PullRequestCoreDevAuthorIssueNotAssigned24h: u32 = 0b00000010;

/// Bitflag indicating an issue has been incorrectly assigned
/// for at least 72h and an appropriate action has been taken
pub const PullRequestCoreDevAuthorIssueNotAssigned72h: u32 = 0b00000100;

pub enum CommitType {
	Update,
	Delete,
	None,
}

impl Default for CommitType {
	fn default() -> CommitType {
		CommitType::None
	}
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum IssueProjectState {
	Confirmed,
	Unconfirmed,
	Denied,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IssueProject {
	pub state: IssueProjectState,
	pub actor_login: String,
	pub project_column_id: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalState {
	key: Vec<u8>,
	actions_taken: u32,
	status_failure_ping: Option<SystemTime>,
	issue_not_assigned_ping: Option<SystemTime>,
	issue_no_project_ping: Option<SystemTime>,
	issue_no_project_npings: u64,
	issue_confirm_project_ping: Option<SystemTime>,
	issue_project: Option<IssueProject>,
	last_confirmed_issue_project: Option<IssueProject>,
}

impl LocalState {
	pub fn new(key: Vec<u8>) -> LocalState {
		LocalState {
			key: key,
			actions_taken: NoAction,
			issue_not_assigned_ping: None,
			issue_no_project_ping: None,
			issue_no_project_npings: 0,
			status_failure_ping: None,
			issue_confirm_project_ping: None,
			issue_project: None,
			last_confirmed_issue_project: None,
		}
	}

	pub fn get_or_new(db: &DB, k: Vec<u8>) -> Result<LocalState> {
		match db.get_pinned(&k).context(error::Db)?.map(|v| {
			serde_json::from_str::<LocalState>(
				String::from_utf8(v.to_vec()).unwrap().as_str(),
			)
			.context(error::Json)
		}) {
			Some(Ok(entry)) => Ok(entry),
			Some(e) => e,
			None => Ok(LocalState::new(k)),
		}
	}

	pub fn actions_taken(&self) -> u32 {
		self.actions_taken
	}

	pub fn update_actions_taken(&mut self, x: u32, db: &DB) -> Result<()> {
		self.actions_taken = x;
		self.commit_update(db)
	}

	pub fn status_failure_ping(&self) -> Option<&SystemTime> {
		self.status_failure_ping.as_ref()
	}

	pub fn update_status_failure_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &DB,
	) -> Result<()> {
		self.status_failure_ping = x;
		self.commit_update(db)
	}

	pub fn issue_not_assigned_ping(&self) -> Option<&SystemTime> {
		self.issue_not_assigned_ping.as_ref()
	}

	pub fn update_issue_not_assigned_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &DB,
	) -> Result<()> {
		self.issue_not_assigned_ping = x;
		self.commit_update(db)
	}

	pub fn issue_no_project_ping(&self) -> Option<&SystemTime> {
		self.issue_no_project_ping.as_ref()
	}

	pub fn update_issue_no_project_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &DB,
	) -> Result<()> {
		self.issue_no_project_ping = x;
		self.commit_update(db)
	}

	pub fn issue_no_project_npings(&self) -> u64 {
		self.issue_no_project_npings
	}

	pub fn update_issue_no_project_npings(
		&mut self,
		x: u64,
		db: &DB,
	) -> Result<()> {
		self.issue_no_project_npings = x;
		self.commit_update(db)
	}

	pub fn issue_confirm_project_ping(&self) -> Option<&SystemTime> {
		self.issue_confirm_project_ping.as_ref()
	}

	pub fn update_issue_confirm_project_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &DB,
	) -> Result<()> {
		self.issue_confirm_project_ping = x;
		self.commit_update(db)
	}

	pub fn issue_project(&self) -> Option<&IssueProject> {
		self.issue_project.as_ref()
	}

	pub fn update_issue_project(
		&mut self,
		x: Option<IssueProject>,
		db: &DB,
	) -> Result<()> {
		self.issue_project = x;
		self.commit_update(db)
	}

	pub fn last_confirmed_issue_project(&self) -> Option<&IssueProject> {
		self.last_confirmed_issue_project.as_ref()
	}

	pub fn update_last_confirmed_issue_project(
		&mut self,
		x: Option<IssueProject>,
		db: &DB,
	) -> Result<()> {
		self.last_confirmed_issue_project = x;
		self.commit_update(db)
	}

	/// delete this entry from the db
	pub fn delete(&self, db: &DB) -> Result<()> {
		db.delete(&self.key).context(error::Db)
	}

	/// commit this entry to the db
	fn commit_update(&self, db: &DB) -> Result<()> {
		db.delete(&self.key).context(error::Db)?;
		db.put(
			&self.key,
			serde_json::to_string(self).context(error::Json)?.as_bytes(),
		)
		.context(error::Db)
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
			0b0000_0110
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
