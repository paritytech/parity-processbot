mod auth;
pub mod bamboo;
pub mod cmd;
mod macros;
#[macro_use]
pub mod companion;
pub mod config;
pub mod constants;
pub mod error;
#[macro_use]
pub mod github;
pub mod github_bot;
pub mod gitlab_bot;
pub mod http;
pub mod logging;
pub mod matrix;
pub mod matrix_bot;
pub mod performance;
pub mod process;
pub mod rebase;
pub mod server;
pub mod setup;
pub mod utils;
pub mod webhook;

pub type Result<T, E = error::Error> = std::result::Result<T, E>;

pub enum Status {
	Success,
	Pending,
	Failure,
}
