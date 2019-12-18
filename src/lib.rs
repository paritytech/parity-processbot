pub mod bot;
pub mod error;
pub mod github;

pub type Result<T> = std::result::Result<T, error::Error>;
