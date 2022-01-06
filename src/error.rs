use snafu::Snafu;

// TODO this really should be struct { owner, repo, number }
pub type IssueDetails = (String, String, i64);

// TODO this really should be struct { repository_url, owner, repo, number }
pub type IssueDetailsWithRepositoryURL = (String, String, String, i64);

#[derive(Debug)]
pub struct CompanionDetailsWithErrorMessage {
	pub owner: String,
	pub repo: String,
	pub number: i64,
	pub html_url: String,
	pub msg: String,
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("WithIssue: {}", source))]
	WithIssue {
		source: Box<Error>,
		issue: IssueDetails,
	},

	#[snafu(display("Checks failed for {}", commit_sha))]
	ChecksFailed {
		commit_sha: String,
	},

	#[snafu(display("Head SHA changed from {} to {}", expected, actual))]
	HeadChanged {
		expected: String,
		actual: String,
	},

	#[snafu(display("Missing approval."))]
	Approval {
		errors: Vec<String>,
		html_url: String,
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

	#[snafu(display("Base64: {}", source))]
	Base64 {
		source: base64::DecodeError,
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

	#[snafu(display("Failed to merge companions: {:?}", errors))]
	CompanionsFailedMerge {
		errors: Vec<CompanionDetailsWithErrorMessage>,
	},
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
	pub fn stops_merge_attempt(&self) -> bool {
		match self {
			Self::WithIssue { source, .. } => source.stops_merge_attempt(),
			Self::MergeFailureWillBeSolvedLater { .. } => false,
			_ => true,
		}
	}
}
