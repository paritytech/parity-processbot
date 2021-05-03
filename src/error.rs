use snafu::Snafu;

// TODO this really should be struct { repository, owner, number }
pub type IssueDetails = (String, String, i64);

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("WithIssue: {}", source))]
	WithIssue {
		source: Box<Error>,
		issue: IssueDetails,
	},

	#[snafu(display("Field is missing: {}", field))]
	MissingField {
		field: String,
	},

	#[snafu(display("Error merging: {}", source))]
	Merge {
		source: Box<Error>,
		commit_sha: String,
		pr_url: String,
		owner: String,
		repo_name: String,
		pr_number: i64,
		created_approval_id: Option<i64>,
	},

	#[snafu(display("Companion update failed: {}", source))]
	CompanionUpdate {
		source: Box<Error>,
	},

	#[snafu(display("Rebase failed: {}", source))]
	Rebase {
		source: Box<Error>,
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

	#[snafu(display("Error getting organization membership: {}", source))]
	OrganizationMembership {
		source: Box<Error>,
	},

	#[snafu(display("Error getting process info: {}", source))]
	ProcessFile {
		source: Box<Error>,
	},

	#[snafu(display("Missing process info."))]
	ProcessInfo {
		errors: Option<Vec<String>>,
	},

	#[snafu(display("Missing approval."))]
	Approval {
		errors: Option<Vec<String>>,
	},

	#[snafu(display("{}", msg))]
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
	#[snafu(display("Http: {}", source))]
	Http {
		source: reqwest::Error,
	},

	/// An error occurred in a Tokio call.
	#[snafu(display("Tokio: {}", source))]
	Tokio {
		source: tokio::io::Error,
	},

	#[snafu(display("IO: {}", source))]
	StdIO {
		source: std::io::Error,
	},

	/// Data requested was not found or valid.
	#[snafu(display("Missing data"))]
	MissingData {},

	/// An error occurred while retrieving or setting values in Rocks DB.
	#[snafu(display("Db: {}", source))]
	Db {
		source: rocksdb::Error,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Utf8: {}", source))]
	Utf8 {
		source: std::string::FromUtf8Error,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Json: {}", source))]
	Json {
		source: serde_json::Error,
	},

	/// An error occurred while parsing TOML.
	#[snafu(display("Base64: {}", source))]
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

	#[snafu(display("Bincode: {}", source))]
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

	#[snafu(display("Error was skipped",))]
	Skipped {},
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

impl From<curl::Error> for Error {
	fn from(value: curl::Error) -> Self {
		Error::Curl {
			status: value.code(),
			body: value.extra_description().map(ToOwned::to_owned),
		}
	}
}
