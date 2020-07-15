use snafu::ResultExt;
use std::time::{Instant};
use tokio::process::Command;

use crate::{error::*, github_bot::GithubBot, Result};

pub async fn performance_regression(
	github_bot: &GithubBot,
	owner: &str,
	repo: &str,
	branch: &str,
) -> Result<Option<u128>> {
	let token = github_bot.client.auth_key().await?;
	let mut reg = None;
	Command::new("git")
		.arg("clone")
		.arg("-vb")
		.arg(branch)
		.arg(format!(
			"https://x-access-token:{token}@github.com/{owner}/{repo}.git",
			token = token,
			owner = owner,
			repo = repo,
		))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	let merge_master = Command::new("git")
		.arg("merge")
		.arg("origin/master")
		.arg("--no-edit")
		.current_dir(format!("./{}", repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	if merge_master.success() {
		// cargo run --release -p node-bench -- node::import::wasm::sr25519 --json
		let now = Instant::now();
		Command::new("cargo")
			.arg("run")
			.arg("--release")
			.arg("-p")
			.arg("node-bench")
			.arg("--quiet")
			.arg("node::import::wasm::sr25519")
			.arg("--json")
			.current_dir(format!("./{}", repo))
			.spawn()
			.context(Tokio)?
			.await
			.context(Tokio)?;
		reg = Some(now.elapsed().as_millis());
	}
	Command::new("rm")
		.arg("-rf")
		.arg(repo)
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	Ok(reg)
}
