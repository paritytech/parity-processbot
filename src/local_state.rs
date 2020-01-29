// TODO: Move bitflags to use `bitflags` crate.
#![allow(non_upper_case_globals)]
use crate::db::DBEntry;
use crate::{github, Result};
use parking_lot::RwLock;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::SystemTime};

/// Bitflag indicating no action has been taken
pub const NoAction: u32 = 0b00000000;

/// Bitflag indicating an issue has been incorrectly assigned
/// for at least 24h and an appropriate action has been taken
pub const PullRequestCoreDevAuthorIssueNotAssigned24h: u32 = 0b00000010;

/// Bitflag indicating an issue has been incorrectly assigned
/// for at least 72h and an appropriate action has been taken
pub const PullRequestCoreDevAuthorIssueNotAssigned72h: u32 = 0b00000100;

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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalState {
	pub key: Vec<u8>,
	actions_taken: u32,
	status_failure_ping: Option<SystemTime>,
	issue_not_assigned_ping: Option<SystemTime>,
	issue_no_project_ping: Option<SystemTime>,
	issue_no_project_npings: u64,
	issue_confirm_project_ping: Option<SystemTime>,
	issue_project: Option<IssueProject>,
	last_confirmed_issue_project: Option<IssueProject>,
	reviews_requested_ping: Option<SystemTime>,
	reviews_requested_npings: u64,
	reviews: HashMap<String, github::ReviewState>,
	private_reviews_requested: HashMap<String, SystemTime>,
	private_review_reminder_npings: HashMap<String, u64>,
	public_reviews_requested: HashMap<String, SystemTime>,
	public_review_reminder_npings: HashMap<String, u64>,
}

impl Default for LocalState {
	fn default() -> LocalState {
		LocalState {
			key: vec![],
			actions_taken: NoAction,
			issue_not_assigned_ping: None,
			issue_no_project_ping: None,
			issue_no_project_npings: 0,
			status_failure_ping: None,
			issue_confirm_project_ping: None,
			issue_project: None,
			last_confirmed_issue_project: None,
			reviews_requested_ping: None,
			reviews_requested_npings: 0,
			reviews: HashMap::new(),
			private_reviews_requested: HashMap::new(),
			private_review_reminder_npings: HashMap::new(),
			public_reviews_requested: HashMap::new(),
			public_review_reminder_npings: HashMap::new(),
		}
	}
}

impl LocalState {
	pub fn actions_taken(&self) -> u32 {
		self.actions_taken
	}

	pub fn update_actions_taken(
		&mut self,
		x: u32,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.actions_taken = x;
		self.update(db, &self.key)
	}

	pub fn status_failure_ping(&self) -> Option<&SystemTime> {
		self.status_failure_ping.as_ref()
	}

	pub fn update_status_failure_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.status_failure_ping = x;
		self.update(db, &self.key)
	}

	pub fn issue_not_assigned_ping(&self) -> Option<&SystemTime> {
		self.issue_not_assigned_ping.as_ref()
	}

	pub fn update_issue_not_assigned_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.issue_not_assigned_ping = x;
		self.update(db, &self.key)
	}

	pub fn issue_no_project_ping(&self) -> Option<&SystemTime> {
		self.issue_no_project_ping.as_ref()
	}

	pub fn update_issue_no_project_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.issue_no_project_ping = x;
		self.update(db, &self.key)
	}

	pub fn issue_no_project_npings(&self) -> u64 {
		self.issue_no_project_npings
	}

	pub fn update_issue_no_project_npings(
		&mut self,
		x: u64,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.issue_no_project_npings = x;
		self.update(db, &self.key)
	}

	pub fn issue_confirm_project_ping(&self) -> Option<&SystemTime> {
		self.issue_confirm_project_ping.as_ref()
	}

	pub fn update_issue_confirm_project_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.issue_confirm_project_ping = x;
		self.update(db, &self.key)
	}

	pub fn issue_project(&self) -> Option<&IssueProject> {
		self.issue_project.as_ref()
	}

	pub fn update_issue_project(
		&mut self,
		x: Option<IssueProject>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.issue_project = x;
		self.update(db, &self.key)
	}

	pub fn last_confirmed_issue_project(&self) -> Option<&IssueProject> {
		self.last_confirmed_issue_project.as_ref()
	}

	pub fn update_last_confirmed_issue_project(
		&mut self,
		x: Option<IssueProject>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.last_confirmed_issue_project = x;
		self.update(db, &self.key)
	}

	pub fn reviews_requested_ping(&self) -> Option<&SystemTime> {
		self.reviews_requested_ping.as_ref()
	}

	pub fn update_reviews_requested_ping(
		&mut self,
		x: Option<SystemTime>,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.reviews_requested_ping = x;
		self.update(db, &self.key)
	}

	pub fn reviews_requested_npings(&self) -> u64 {
		self.reviews_requested_npings
	}

	pub fn update_reviews_requested_npings(
		&mut self,
		x: u64,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.reviews_requested_npings = x;
		self.update(db, &self.key)
	}

	pub fn review_from_user(
		&self,
		user_login: &str,
	) -> Option<&github::ReviewState> {
		self.reviews.get(user_login)
	}

	pub fn update_review(
		&mut self,
		user_login: String,
		review: github::ReviewState,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.reviews.insert(user_login, review);
		self.update(db, &self.key)
	}

	pub fn private_review_requested_from_user(
		&self,
		user_login: &str,
	) -> Option<&SystemTime> {
		self.private_reviews_requested.get(user_login)
	}

	pub fn update_private_review_requested(
		&mut self,
		user_login: String,
		t: SystemTime,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.private_reviews_requested.insert(user_login, t);
		self.update(db, &self.key)
	}

	pub fn private_review_reminder_npings(
		&self,
		user_login: &str,
	) -> Option<&u64> {
		self.private_review_reminder_npings.get(user_login)
	}

	pub fn update_private_review_reminder_npings(
		&mut self,
		user_login: String,
		npings: u64,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.private_review_reminder_npings
			.insert(user_login, npings);
		self.update(db, &self.key)
	}

	pub fn public_review_requested_from_user(
		&self,
		user_login: &str,
	) -> Option<&SystemTime> {
		self.public_reviews_requested.get(user_login)
	}

	pub fn update_public_review_requested(
		&mut self,
		user_login: String,
		t: SystemTime,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.public_reviews_requested.insert(user_login, t);
		self.update(db, &self.key)
	}

	pub fn public_review_reminder_npings(
		&self,
		user_login: &str,
	) -> Option<&u64> {
		self.public_review_reminder_npings.get(user_login)
	}

	pub fn update_public_review_reminder_npings(
		&mut self,
		user_login: String,
		npings: u64,
		db: &Arc<RwLock<DB>>,
	) -> Result<()> {
		self.public_review_reminder_npings
			.insert(user_login, npings);
		self.update(db, &self.key)
	}
}

impl DBEntry for LocalState {
	fn with_key(self, k: Vec<u8>) -> LocalState {
		let mut s = self;
		s.key = k;
		s
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
