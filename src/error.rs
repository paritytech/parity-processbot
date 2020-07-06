use crate::Result;
use snafu::{Backtrace, Snafu};

type IssueDetails = Option<(String, String, i64)>;

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("Source: {}", source))]
	WithIssue {
		source: Box<Error>,
		issue: IssueDetails,
	},

	#[snafu(display("Checks failed for {}", commit_sha))]
	ChecksFailed {
		commit_sha: String,
	},

	#[snafu(display("Head SHA changed from {}", commit_sha))]
	HeadChanged {
		commit_sha: String,
	},

	#[snafu(display("Error getting organization membership: {}", source))]
	OrganizationMembership {
		source: Box<Error>,
	},

	#[snafu(display("Error: {}", msg))]
	Message {
		msg: String,
	},

	/// An error occurred with an integration service (e.g. GitHub).
	#[snafu(display(
		"Status code: {}\nBody:\n{:#?}\nBacktrace:\n{}",
		status,
		body,
		backtrace
	))]
	Response {
		status: reqwest::StatusCode,
		body: serde_json::Value,
		backtrace: Backtrace,
	},

	/// An error occurred while sending or receiving a HTTP request or response
	/// respectively.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Http {
		source: reqwest::Error,
		backtrace: Backtrace,
	},

	/// Data requested was not found or valid.
	#[snafu(display("Backtrace:\n{}", backtrace))]
	MissingData {
		backtrace: Backtrace,
	},

	/// An error occurred while retrieving or setting values in Rocks DB.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Db {
		source: rocksdb::Error,
		backtrace: Backtrace,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Utf8 {
		source: std::string::FromUtf8Error,
		backtrace: Backtrace,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Json {
		source: serde_json::Error,
		backtrace: Backtrace,
	},

	/// An error occurred while parsing TOML.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Toml {
		source: toml::de::Error,
		backtrace: Backtrace,
	},

	/// An error occurred while parsing TOML.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Base64 {
		source: base64::DecodeError,
		backtrace: Backtrace,
	},

	/// An error occurred with a curl request.
	#[snafu(display("Status code: {}\nBody:\n{:#?}", status, body))]
	Curl {
		status: curl_sys::CURLcode,
		body: Option<String>,
	},

	Jwt {
		source: jsonwebtoken::errors::Error,
	},

	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Bincode {
		source: bincode::Error,
		backtrace: Backtrace,
	},
}

impl Error {
	pub fn map_issue(self, issue: IssueDetails) -> Self {
		Self::WithIssue {
			source: Box::new(self),
			issue: issue,
		}
	}
}

impl Error {
	pub fn map_issue(self, issue: IssueDetails) -> Self {
		Self::WithIssue {
			source: Box::new(self),
			issue: issue,
		}
	}
}

/// Maps a curl error into a crate::error::Error.
pub fn map_curl_error<T>(err: curl::Error) -> Result<T> {
	Err(Error::Curl {
		status: err.code(),
		body: err.extra_description().map(|s| s.to_owned()),
	})
}
