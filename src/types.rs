use crate::{error::Error, github_bot::GithubBot};
use rocksdb::DB;

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub enum Status {
	Success,
	Pending,
	Failure,
}

pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub webhook_secret: String,
}

#[derive(Debug)]
pub struct IssueDetails {
	owner: String,
	repo: String,
	number: usize,
}

#[derive(Debug)]
pub struct IssueDetailsWithRepositoryURL {
	issue: IssueDetails,
	repo_url: String,
}
