pub mod bamboo;
pub mod bots;
pub mod constants;
pub mod db;
pub mod error;
pub mod github;
pub mod github_bot;
pub mod issue;
pub mod matrix;
pub mod matrix_bot;
pub mod project;
pub mod pull_request;
pub mod repository;

pub type Result<T> = std::result::Result<T, error::Error>;
