use snafu::Snafu;

#[derive(Debug)]
pub struct PullRequestDetails {
	pub owner: String,
	pub repo: String,
	pub number: i64,
}

#[derive(PartialEq, Debug, Clone)]
pub struct PullRequestDetailsWithHtmlUrl {
	pub html_url: String,
	pub owner: String,
	pub repo: String,
	pub number: i64,
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("WithIssue: {}", source))]
	WithPullRequestDetails {
		source: Box<Error>,
		details: PullRequestDetails,
	},

	#[snafu(display("Checks failed for {}", commit_sha))]
	ChecksFailed {
		commit_sha: String,
	},

	#[snafu(display("Statuses failed for {}", commit_sha))]
	StatusesFailed {
		commit_sha: String,
	},

	#[snafu(display("Head SHA changed from {} to {}", expected, actual))]
	HeadChanged {
		expected: String,
		actual: String,
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

	#[snafu(display(
		"Encountered merge failure (would be solved later): {}",
		msg
	))]
	MergeFailureWillBeSolvedLater {
		msg: String,
	},
}

impl Error {
	pub fn with_pr_details(self, details: PullRequestDetails) -> Self {
		match self {
			Self::WithPullRequestDetails { .. } => self,
			_ => Self::WithPullRequestDetails {
				source: Box::new(self),
				details,
			},
		}
	}
	pub fn stops_merge_attempt(&self) -> bool {
		match self {
			Self::WithPullRequestDetails { source, .. } => {
				source.stops_merge_attempt()
			}
			Self::MergeFailureWillBeSolvedLater { .. } => false,
			_ => true,
		}
	}
}
