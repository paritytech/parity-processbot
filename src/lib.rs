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
pub mod http;
pub mod rebase;
pub mod server;
pub mod vanity_service;
pub mod webhook;

pub type Result<T, E = error::Error> = std::result::Result<T, E>;

pub enum Status {
	Success,
	Pending,
	Failure,
}

pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub webhook_secret: String,
}
