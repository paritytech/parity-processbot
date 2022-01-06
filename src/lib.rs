#![forbid(unsafe_code)]
#![allow(clippy::blocks_in_if_conditions)]
#![allow(clippy::too_many_arguments)]

use serde::{Deserialize, Serialize};

pub mod cmd;
mod macros;
#[macro_use]
pub mod companion;
pub mod config;
pub mod constants;
pub mod error;
#[macro_use]
pub mod github;
pub mod github_bot;
pub mod http;
pub mod rebase;
pub mod server;
pub mod utils;
pub mod vanity_service;
pub mod webhook;

pub type Result<T, E = error::Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub enum Status {
	Success,
	Pending,
	Failure,
}

#[derive(Debug)]
pub enum MergeCommentCommand {
	Normal,
	Force,
}
#[derive(Debug)]
pub enum CommentCommand {
	Merge(MergeCommentCommand),
	CancelMerge,
	Rebase,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceholderDeserializationItem {}

pub enum MergeCancelOutcome {
	ShaNotFound,
	WasCancelled,
	WasNotCancelled,
}

pub enum MergeAllowedOutcome {
	Allowed,
	GrantApprovalForRole(String),
	Disallowed(String),
}
