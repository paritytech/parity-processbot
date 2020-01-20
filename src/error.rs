use crate::Result;
use snafu::{Backtrace, GenerateBacktrace, Snafu};

pub fn unwrap_field<T>(x: Option<T>) -> Result<T> {
	x.ok_or(Error::MissingData {
		backtrace: snafu::Backtrace::generate(),
	})
}

#[derive(Debug, Snafu)]
#[snafu(visibility = "pub")]
pub enum Error {
	/// An error occurred while sending or receiving a HTTP request or response
	/// respectively.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Http {
		source: reqwest::Error,
		backtrace: Backtrace,
	},

	/// Data requested was not found or valid.
	#[snafu(display("Backtrace:\n{}", backtrace))]
	MissingData { backtrace: Backtrace },

	/// An error occurred while retrieving or setting values in Rocks DB.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Db {
		source: rocksdb::Error,
		backtrace: Backtrace,
	},

	/// An error occurred while parsing or serializing JSON.
	#[snafu(display("Source: {}\nBacktrace:\n{}", source, backtrace))]
	Json {
		source: serde_json::Error,
		backtrace: Backtrace,
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

	/// An error occurred with a curl request.
	#[snafu(display("Status code: {}\nBody:\n{:#?}", status, body))]
	Curl {
		status: curl_sys::CURLcode,
		body: Option<String>,
	},
}

/// Maps a curl error into a crate::error::Error.
pub fn map_curl_error<T>(err: curl::Error) -> Result<T> {
	Err(Error::Curl {
		status: err.code(),
		body: err.extra_description().map(|s| s.to_owned()),
	})
}
