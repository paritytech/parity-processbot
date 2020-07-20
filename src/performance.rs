use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::time::Instant;
use tokio::process::Command;

use crate::{error::*, github_bot::GithubBot, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BenchResult {
	name: String,
	raw_average: i64,
	average: i64,
}

pub async fn regression(
	github_bot: &GithubBot,
	owner: &str,
	repo: &str,
	branch: &str,
) -> Result<Option<i64>> {
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
		let res: Vec<BenchResult> = serde_json::from_str(&String::from_utf8_lossy(&Command::new("cargo")
			.arg("run")
			.arg("--release")
			.arg("-p")
			.arg("node-bench")
			.arg("--quiet")
			.arg("node::import::wasm::sr25519::transfer_keep_alive::rocksdb::medium")
			.arg("--json")
			.current_dir(format!("./{}", repo))
			.output()
			.await
			.context(Tokio)?
            .stdout)).expect("bench result");
		dbg!(res);
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
