#![forbid(unsafe_code)]
#![allow(clippy::blocks_in_if_conditions)]
#![allow(clippy::too_many_arguments)]

pub mod macros;
pub mod shell;
#[macro_use]
pub mod companion;
pub mod config;
pub mod constants;
pub mod error;
#[macro_use]
pub mod github;
pub mod bot;
pub mod core;
pub mod git_ops;
pub mod gitlab;
pub mod merge_request;
pub mod server;
pub mod types;
pub mod vanity_service;
