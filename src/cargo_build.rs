use snafu::ResultExt;
use tokio::process::Command;

use crate::{error::*, Result};

/// Clone and build a rust project.
pub async fn clone_build(
	token: &str,
	base_owner: &str,
	base_repo: &str,
) -> Result<()> {
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
	// build release
	log::info!("Building.");
	Command::new("cargo")
		.arg("build")
		.arg("--release")
		.arg("--quiet")
		.current_dir(format!("./{}", base_repo))
		.output()
		.await
		.context(Tokio)?;
	Ok(())
}
