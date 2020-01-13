pub mod bamboo;
pub mod bots;
pub mod constants;
pub mod db;
pub mod duration_ticks;
pub mod error;
pub mod github;
pub mod github_bot;
pub mod http;
pub mod issue;
pub mod matrix;
pub mod matrix_bot;
pub mod project_info;
pub mod pull_request;

pub type Result<T> = std::result::Result<T, error::Error>;
