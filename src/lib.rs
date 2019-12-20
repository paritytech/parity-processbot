pub mod github_bot;
pub mod matrix_bot;
pub mod pull_request;
//pub mod team;
//pub mod project;
pub mod bots;
pub mod db;
pub mod error;
pub mod github;
pub mod matrix;
pub mod repository;
pub mod user;
pub mod bamboo;

pub type Result<T> = std::result::Result<T, error::Error>;
