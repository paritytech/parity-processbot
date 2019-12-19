pub mod github_bot;
pub mod issue;
pub mod matrix_bot;
pub mod pull_request;
pub mod review_request;
//pub mod team;
//pub mod project;
pub mod db;
pub mod user;
pub mod error;
pub mod github;
pub mod matrix;
pub mod repository;
pub mod review;

pub type Result<T> = std::result::Result<T, error::Error>;
