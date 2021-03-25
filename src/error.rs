use snafu::Snafu;

type IssueDetails = Option<(String, String, i64)>;

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("Source: {}", source))]
	WithIssue {
		source: Box<Error>,
		issue: IssueDetails,
	},

	#[snafu(display("Error updating companion: {}", source))]
	Companion {
		source: Box<Error>,
	},

	#[snafu(display("Error merging: {}", source))]
	Merge {
		source: Box<Error>,
		commit_sha: String,
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

	#[snafu(display("Error getting process info: {}", source))]
	ProcessFile {
		source: Box<Error>,
	},

	#[snafu(display("Missing process info."))]
	ProcessInfo {},

	#[snafu(display("Missing approval."))]
	Approval {},

	#[snafu(display("Error: {}", msg))]
	Message {
		msg: String,
	},

	/// An error occurred with an integration service (e.g. GitHub).
	#[snafu(display("Status code: {}\nBody:\n{:#?}", status, body,))]
	Response {
		status: reqwest::StatusCode,
		body: serde_json::Value,
	},

	/// An error occurred while sending or receiving a HTTP request or response
	/// respectively.
	#[snafu(display("Source: {}", source))]
	Http {
		source: reqwest::Error,
	},

	/// An error occurred in a Tokio call.
	#[snafu(display("Source: {}", source))]
	Tokio {
		source: tokio::io::Error,
	},

	/// Data requested was not found or valid.
	#[snafu(display("Missing data"))]
	MissingData {},

	/// An error occurred while retrieving or setting values in Rocks DB.
	#[snafu(display("Source: {}", source))]
	Db {
		source: rocksdb::Error,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Source: {}", source))]
	Utf8 {
		source: std::string::FromUtf8Error,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Source: {}", source))]
	Json {
		source: serde_json::Error,
	},

	/// An error occurred while parsing TOML.
	#[snafu(display("Source: {}", source))]
	Toml {
		source: toml::de::Error,
	},

	/// An error occurred while parsing TOML.
	#[snafu(display("Source: {}", source))]
	Base64 {
		source: base64::DecodeError,
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

	#[snafu(display("Source: {}", source))]
	Bincode {
		source: bincode::Error,
	},

	GitlabJobNotFound {
		commit_sha: String,
	},

	// Gitlab API responded with an HTTP status >299 to POST /jobs/<id>/play
	#[snafu(display(
		"Starting CI job {} failed with HTTP status {} and body: {}",
		url,
		status,
		body
	))]
	StartingGitlabJobFailed {
		url: String,
		status: u32,
		body: String,
	},

	// Gitlab API responded with an HTTP status >299 to requests other than POST /jobs/<id>/play
	#[snafu(display(
		"{} {} failed with HTTP status {} and body: {}",
		method,
		url,
		status,
		body
	))]
	GitlabApi {
		method: String,
		url: String,
		status: u32,
		body: String,
	},

	#[snafu(display("Failed parsing URL: {}", source))]
	ParseUrl {
		source: url::ParseError,
	},

	#[snafu(display("URL {} cannot be base", url))]
	UrlCannotBeBase {
		url: String,
	},

	#[snafu(display(
		"Cmd '{}' failed with status {:?}; output: {}",
		cmd,
		status_code,
		err
	))]
	CommandFailed {
		cmd: String,
		status_code: Option<i32>,
		err: String,
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

impl From<curl::Error> for Error {
	fn from(value: curl::Error) -> Self {
		Error::Curl {
			status: value.code(),
			body: value.extra_description().map(ToOwned::to_owned),
		}
	}
}
