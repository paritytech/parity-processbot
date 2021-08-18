use snafu::Snafu;

pub mod http_error;

pub struct IssueDetails {
	repository: string,
	owner: string,
	number: usize,
}

pub struct IssueDetailsWithRepositoryURL {
	details: IssueDetails,
	repository_url: String,
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	#[snafu(display("WithIssue: {}", source))]
	WithIssue {
		source: Box<Error>,
		issue: IssueDetails,
	},

	#[snafu(display("Error merging: {}", source))]
	Merge {
		source: Box<Error>,
		commit_sha: String,
		pr_url: String,
		owner: String,
		repo_name: String,
		pr_number: usize,
		created_approval_id: Option<usize>,
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

	#[snafu(display("Base64: {}", source))]
	Base64 {
		source: base64::DecodeError,
	},

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
