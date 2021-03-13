use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tokio::process::Command;

use crate::{error::*, github_bot::GithubBot, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BenchResult {
	name: String,
	raw_average: i64,
	average: i64,
}

/// Return the factor by which performance deteriorates on the head branch.
/// IE, return `head_time / base_time`.
pub async fn regression(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	head_branch: &str,
) -> Result<Option<f64>> {
	let res = regression_inner(
		github_bot,
		owner,
		owner_repo,
		contributor,
		contributor_repo,
		head_branch,
	)
	.await;
	// checkout origin master
	log::info!("Checking out master.");
	Command::new("git")
		.arg("checkout")
		.arg("master")
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// delete temp branch
	log::info!("Deleting head branch.");
	Command::new("git")
		.arg("branch")
		.arg("-D")
		.arg(format!("{}", head_branch))
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// remove temp remote
	log::info!("Removing temp remote.");
	Command::new("git")
		.arg("remote")
		.arg("remove")
		.arg("temp")
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	res
}

async fn regression_inner(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	branch: &str,
) -> Result<Option<f64>> {
	let token = github_bot.client.auth_key().await?;

	let (owner_remote_address, _) =
		github_bot.get_fetch_components(owner, owner_repo, &token);
	// clone in case the local clone doesn't exist
	log::info!("Cloning repo.");
	Command::new("git")
		.arg("clone")
		.arg("-v")
		.arg(&owner_remote_address)
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// checkout origin master
	log::info!("Checking out master.");
	Command::new("git")
		.arg("checkout")
		.arg("master")
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// pull origin master
	log::info!("Pulling master.");
	Command::new("git")
		.arg("pull")
		.arg("-v")
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// bench origin master
	log::info!("Running bench.");
	let bench = Command::new("cargo")
		.arg("run")
		.arg("--release")
		.arg("-vp")
		.arg("node-bench")
		.arg("--quiet")
		.arg(
			"node::import::wasm::sr25519::transfer_keep_alive::rocksdb::medium",
		)
		.arg("--json")
		.current_dir(format!("./{}", owner_repo))
		.output()
		.await
		.context(Tokio)?;
	let base_res: Vec<BenchResult> =
		serde_json::from_str(&String::from_utf8_lossy(&bench.stdout))
			.context(Json)?;
	let base_reg = base_res.first().map(|r| r.average);
	let mut head_reg = None;

	let (contributor_remote_address, _) =
		github_bot.get_fetch_components(contributor, contributor_repo, &token);
	log::info!("Adding temp remote.");
	Command::new("git")
		.arg("remote")
		.arg("add")
		.arg("temp")
		.arg(&contributor_remote_address)
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// fetch temp
	log::info!("Fetching temp.");
	Command::new("git")
		.arg("fetch")
		.arg("temp")
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// checkout temp branch
	log::info!("Checking out head branch.");
	let checkout = Command::new("git")
		.arg("checkout")
		.arg("-b")
		.arg(format!("{}", branch))
		.arg(format!("temp/{}", branch))
		.current_dir(format!("./{}", owner_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	if checkout.success() {
		// merge origin master
		log::info!("Merging master.");
		let merge_master = Command::new("git")
			.arg("merge")
			.arg("origin/master")
			.arg("--no-edit")
			.current_dir(format!("./{}", owner_repo))
			.spawn()
			.context(Tokio)?
			.await
			.context(Tokio)?;
		if merge_master.success() {
			// bench temp branch
			let head_res: Vec<BenchResult> = serde_json::from_str(&String::from_utf8_lossy(&Command::new("cargo")
				.arg("run")
				.arg("--release")
				.arg("-p")
				.arg("node-bench")
				.arg("--quiet")
				.arg("node::import::wasm::sr25519::transfer_keep_alive::rocksdb::medium")
				.arg("--json")
				.current_dir(format!("./{}", owner_repo))
				.output()
				.await
				.context(Tokio)?
				.stdout)).context(Json)?;
			head_reg = head_res.first().map(|r| r.average);
		} else {
			// abort merge
			log::info!("Aborting merge.");
			Command::new("git")
				.arg("merge")
				.arg("--abort")
				.current_dir(format!("./{}", owner_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
		}
	}
	// calculate regression
	let reg = base_reg
		.map(|base| head_reg.map(|head| head as f64 / base as f64))
		.flatten();
	Ok(reg)
}
