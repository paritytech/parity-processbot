use snafu::ResultExt;
use tokio::process::Command;

use crate::{error::*, github::GithubClient, types::Result};

pub async fn rebase(
	gh_client: &GithubClient,
	base_owner: &str,
	base_repo: &str,
	head_owner: &str,
	head_repo: &str,
	branch: &str,
) -> Result<()> {
	let res = rebase_inner(
		gh_client, base_owner, base_repo, head_owner, head_repo, branch,
	)
	.await;
	// checkout origin master
	log::info!("Checking out master.");
	Command::new("git")
		.arg("checkout")
		.arg("master")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// delete temp branch
	log::info!("Deleting head branch.");
	Command::new("git")
		.arg("branch")
		.arg("-D")
		.arg(branch)
		.current_dir(format!("./{}", base_repo))
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
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	res
}

async fn rebase_inner(
	gh_client: &GithubClient,
	base_owner: &str,
	base_repo: &str,
	head_owner: &str,
	head_repo: &str,
	branch: &str,
) -> Result<()> {
	let token = gh_client.client.auth_key().await?;
	// clone in case the local clone doesn't exist
	log::info!("Cloning repo.");
	Command::new("git")
		.arg("clone")
		.arg("-v")
		.arg(format!(
			"https://x-access-token:{token}@github.com/{owner}/{repo}.git",
			token = token,
			owner = base_owner,
			repo = base_repo,
		))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// add temp remote
	log::info!("Adding temp remote.");
	Command::new("git")
		.arg("remote")
		.arg("add")
		.arg("temp")
		.arg(format!(
			"https://x-access-token:{token}@github.com/{owner}/{repo}.git",
			token = token,
			owner = head_owner,
			repo = head_repo,
		))
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// fetch temp
	log::info!("Fetching temp.");
	Command::new("git")
		.arg("fetch")
		.arg("temp")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// checkout temp branch
	log::info!("Checking out head branch.");
	let checkout = Command::new("git")
		.arg("checkout")
		.arg("-b")
		.arg(branch)
		.arg(format!("temp/{}", branch))
		.current_dir(format!("./{}", base_repo))
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
			.arg("--no-ff")
			.arg("--no-edit")
			.current_dir(format!("./{}", base_repo))
			.spawn()
			.context(Tokio)?
			.await
			.context(Tokio)?;
		if merge_master.success() {
			// push
			log::info!("Pushing changes.");
			Command::new("git")
				.arg("push")
				.arg("temp")
				.arg(branch)
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
		} else {
			// abort merge
			log::info!("Aborting merge.");
			Command::new("git")
				.arg("merge")
				.arg("--abort")
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
		}
	}
	Ok(())
}
