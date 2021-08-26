use super::*;
use crate::error::*;
use snafu::ResultExt;
use tokio::process::Command;

async fn rebase_inner<'a>(&self, args: RebaseArgs<'a>) -> Result<()> {
	let RebaseArgs {
		base_owner,
		base_repo,
		head_owner,
		head_repo,
		branch,
	} = args;
	let token = self.client.auth_key().await?;

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
		.context(error::Tokio)?
		.await
		.context(error::Tokio)?;
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
		.context(error::Tokio)?
		.await
		.context(error::Tokio)?;
	// fetch temp
	log::info!("Fetching temp.");
	Command::new("git")
		.arg("fetch")
		.arg("temp")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(error::Tokio)?
		.await
		.context(error::Tokio)?;
	// checkout temp branch
	log::info!("Checking out head branch.");
	let checkout = Command::new("git")
		.arg("checkout")
		.arg("-b")
		.arg(format!("{}", branch))
		.arg(format!("temp/{}", branch))
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(error::Tokio)?
		.await
		.context(error::Tokio)?;
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
			.context(error::Tokio)?
			.await
			.context(error::Tokio)?;
		if merge_master.success() {
			// push
			log::info!("Pushing changes.");
			Command::new("git")
				.arg("push")
				.arg("temp")
				.arg(format!("{}", branch))
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(error::Tokio)?
				.await
				.context(error::Tokio)?;
		} else {
			// abort merge
			log::info!("Aborting merge.");
			Command::new("git")
				.arg("merge")
				.arg("--abort")
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(error::Tokio)?
				.await
				.context(error::Tokio)?;
		}
	}
	Ok(())
}

impl Bot {
	pub async fn rebase<'a>(&self, args: RebaseArgs<'a>) -> Result<()> {
		let RebaseArgs {
			base_owner,
			base_repo,
			head_owner,
			head_repo,
			branch,
		} = args;
		let rebase_result = rebase_inner(self, args).await;
		log::info!("Checking out master.");
		Command::new("git")
			.arg("checkout")
			.arg("master")
			.current_dir(format!("./{}", base_repo))
			.spawn()
			.context(error::Tokio)?
			.await
			.context(error::Tokio)?;
		// delete temp branch
		log::info!("Deleting head branch.");
		Command::new("git")
			.arg("branch")
			.arg("-D")
			.arg(format!("{}", branch))
			.current_dir(format!("./{}", base_repo))
			.spawn()
			.context(error::Tokio)?
			.await
			.context(error::Tokio)?;
		// remove temp remote
		log::info!("Removing temp remote.");
		Command::new("git")
			.arg("remote")
			.arg("remove")
			.arg("temp")
			.current_dir(format!("./{}", base_repo))
			.spawn()
			.context(error::Tokio)?
			.await
			.context(error::Tokio)?;
		rebase_result
	}
}
