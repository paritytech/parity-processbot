use crate::types::{AppState, IssueDetails, IssueDetailsWithRepositoryURL};
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum MergeError {
	FailureWillBeSolvedLater,
	Error(Error),
}

// This enum is exclusive for unactionable errors which should stop the webhook payload from being
// processed at once.
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("WithIssue: {}", source))]
	WithIssue {
		source: Box<Error>,
		issue: IssueDetails,
	},

	#[snafu(display("Merge attempt failed: {}", source))]
	MergeAttemptFailed {
		source: Box<Error>,
		commit_sha: String,
		created_approval_id: Option<usize>,
		owner: string,
		repo: string,
		pr_number: usize,
	},

	#[snafu(display("Error getting organization membership: {}", source))]
	OrganizationMembership {
		source: Box<Error>,
	},

	#[snafu(display("{}", msg))]
	Message {
		msg: String,
	},

	#[snafu(display("Status code: {}\nBody:\n{:#?}", status, body,))]
	Response {
		status: reqwest::StatusCode,
		body: serde_json::Value,
	},

	#[snafu(display("Http: {}", source))]
	Http {
		source: reqwest::Error,
	},

	#[snafu(display("Tokio: {}", source))]
	Tokio {
		source: tokio::io::Error,
	},

	#[snafu(display("Db: {}", source))]
	Db {
		source: rocksdb::Error,
	},

	#[snafu(display("Utf8: {}", source))]
	Utf8 {
		source: std::string::FromUtf8Error,
	},

	#[snafu(display("Json: {}", source))]
	Json {
		source: serde_json::Error,
	},

	Jwt {
		source: jsonwebtoken::errors::Error,
	},

	#[snafu(display("Bincode: {}", source))]
	Bincode {
		source: bincode::Error,
	},

	#[snafu(display(
		"Command '{}' failed with status {:?}; output: {}",
		cmd,
		status_code,
		err
	))]
	CommandFailed {
		cmd: String,
		status_code: Option<i32>,
		err: String,
	},

	UnregisterPullRequest {
		commit_sha: String,
		message: String,
	},

	Skipped,
}

impl Error {
	pub fn map_issue(self, issue: IssueDetails) -> Self {
		match self {
			Self::WithIssue { source, .. } => Self::WithIssue { source, issue },
			_ => Self::WithIssue {
				source: Box::new(self),
				issue,
			},
		}
	}
}

fn display_errors_along_the_way(errors: Option<Vec<String>>) -> String {
	errors
		.map(|errors| {
			if errors.len() == 0 {
				"".to_string()
			} else {
				format!(
					"The following errors *might* have affected the outcome of this attempt:\n{}",
					errors.iter().map(|e| format!("- {}", e)).join("\n")
				)
			}
		})
		.unwrap_or_else(|| "".to_string())
}

async fn handle_error_inner(err: Error, state: &AppState) -> Option<String> {
	match err {
		Error::MergeAttemptFailed {
			source,
			commit_sha,
			created_approval_id,
			owner,
			repo,
			pr_number
		} => {
			let _ = state.db.delete(commit_sha.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			let github_bot = &state.github_bot;
			if let Some(created_approval_id) = created_approval_id {
				let _ =
					github_bot
						.clear_bot_approval(
							&owner,
							&repo,
							pr_number,
							created_approval_id,
						)
						.await
						.map_err(|e| {
							log::error!("Failed to cleanup a bot review in {} due to: {}", pr_url, e)
						});
			}
			match *source {
				Error::Response { body, status } => Some(format!(
					"Merge failed with response status: {} and body: `{}`",
					status, body
				)),
				Error::Http { source, .. } => Some(format!(
					"Merge failed due to network error:\n\n{}",
					source
				)),
				Error::Message { .. } => {
					Some(format!("Merge failed: {}", *source))
				}
				_ => Some("Merge failed due to unexpected error".to_string()),
			}
		}
		Error::Approval { errors } => Some(format!(
			"Error: Approval criteria was not satisfied.\n\n{}\n\nMerge failed. Check out the [criteria for merge](https://github.com/paritytech/parity-processbot#criteria-for-merge).",
			display_errors_along_the_way(errors),
		)),
		Error::UnregisterPullRequest { commit_sha, message } => {
			let _ = state.db.delete(expected.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			Some(format!("Merge aborted: {}", message))
		}
		Error::ChecksFailed { ref commit_sha } => {
			let _ = state.db.delete(commit_sha.as_bytes()).map_err(|e| {
				log::error!("Error deleting merge request from db: {}", e);
			});
			Some(format!("Merge aborted: {}", err))
		}
		Error::Response {
			body: serde_json::Value::Object(m),
			..
		} => Some(format!("Response error: `{}`", m["message"])),
		Error::OrganizationMembership { .. }
		| Error::CompanionUpdate { .. }
		| Error::Message { .. }
		| Error::Rebase { .. } => Some(format!("Error: {}", err)),
		_ => None,
	}
}

async fn handle_error(err: Error, state: &AppState) {
	match err {
		Error::Skipped { .. } => (),
		e => match e {
			Error::WithIssue {
				source,
				issue: IssueDetails {
					owner,
					repo,
					number,
				},
				..
			} => match *source {
				Error::Skipped { .. } => (),
				e => {
					log::error!("handle_error: {}", e);
					let msg = handle_error_inner(e, state)
						.await
						.unwrap_or_else(|| {
							format!(
								"Unexpected error (at {} server time).",
								chrono::Utc::now().to_string()
							)
						});
					let _ = state
						.github_bot
						.create_issue_comment(&owner, &repo, number, &msg)
						.await
						.map_err(|e| {
							log::error!("Error posting comment: {}", e);
						});
				}
			},
			_ => {
				log::error!("handle_error: {}", e);
				handle_error_inner(e, state).await;
			}
		},
	}
}
