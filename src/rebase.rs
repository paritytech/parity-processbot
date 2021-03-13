use snafu::ResultExt;
use tokio::process::Command;

use crate::{error::*, github_bot::GithubBot, Result};

pub async fn rebase(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	branch: &str,
) -> Result<()> {
	let res = rebase_inner(
		github_bot,
		owner,
		owner_repo,
		contributor,
		contributor_repo,
		branch,
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
		.arg(format!("{}", branch))
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

async fn rebase_inner(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	branch: &str,
) -> Result<()> {
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

	let (contributor_remote_address, _) =
		github_bot.get_fetch_components(contributor, contributor_repo, &token);
	// add temp remote
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
			.arg("--no-ff")
			.arg("--no-edit")
			.current_dir(format!("./{}", owner_repo))
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
				.arg(format!("{}", branch))
				.current_dir(format!("./{}", owner_repo))
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
				.current_dir(format!("./{}", owner_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
		}
	}
	Ok(())
}
