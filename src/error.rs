use snafu::{Backtrace, Snafu};

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
    MissingData { backtrace: Backtrace },

    /// An error occurred while retrieving or setting values in Rocks DB.
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
    #[snafu(display("Status code: {}\nBody:\n{:#?}", status, body))]
    Response {
        status: reqwest::StatusCode,
        body: serde_json::Value,
    },

    /// An error occurred when initialising `Bot`.
    #[snafu(display("Error creating bot: {}", msg))]
    BotCreation { msg: String },
}
