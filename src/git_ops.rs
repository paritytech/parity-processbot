use snafu::ResultExt;

use crate::{
	core::AppState,
	error::*,
	shell::{
		run_cmd, run_cmd_in_cwd, run_cmd_with_output, CommandMessage,
		CommandMessageConfiguration,
	},
	types::Result,
};

pub struct SetupContributorBranchData {
	pub contributor_remote: String,
	pub repo_dir: String,
	pub secrets_to_hide: Option<Vec<String>>,
	pub contributor_remote_branch: String,
}
pub async fn setup_contributor_branch(
	state: &AppState,
	owner: &str,
	owner_repo: &str,
	owner_branch: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
) -> Result<SetupContributorBranchData> {
	let AppState {
		gh_client, config, ..
	} = state;

	/*
		Constantly refresh the token in-between operations, preferably right before
		using it, for avoiding expiration issues. Some operations such as cloning
		repositories might take a long time, thus causing the token to be
		invalidated after it finishes. In any case, the token generation API should
		backed by a cache, thus there's no problem with spamming the refresh calls.
	*/

	let repo_dir = config.repos_path.join(owner_repo);
	let repo_dir_str = if let Some(repo_dir_str) = repo_dir.as_os_str().to_str()
	{
		repo_dir_str
	} else {
		return Err(Error::Message {
			msg: format!(
				"Path {:?} could not be converted to string",
				repo_dir
			),
		});
	};

	if repo_dir.exists() {
		log::info!("{} is already cloned; skipping", owner_repo);
	} else {
		let token = gh_client.auth_token().await?;
		let secrets_to_hide = [token.as_str()];
		let secrets_to_hide = Some(&secrets_to_hide[..]);
		let owner_repository_domain =
			format!("github.com/{}/{}.git", owner, owner_repo);
		let owner_remote_address = format!(
			"https://x-access-token:{}@{}",
			token, owner_repository_domain
		);
		run_cmd_in_cwd(
			"git",
			&["clone", "-v", &owner_remote_address, repo_dir_str],
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
	}

	run_cmd(
		"git",
		&["add", "."],
		&repo_dir,
		CommandMessage::Configured::<'_, &str>(CommandMessageConfiguration {
			secrets_to_hide: None,
			are_errors_silenced: false,
		}),
	)
	.await?;
	run_cmd(
		"git",
		&["reset", "--hard"],
		&repo_dir,
		CommandMessage::Configured::<'_, &str>(CommandMessageConfiguration {
			secrets_to_hide: None,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// The contributor's remote entry might exist from a previous run (not expected for a fresh
	// clone). If that is the case, delete it so that it can be recreated.
	if run_cmd(
		"git",
		&["remote", "get-url", contributor],
		&repo_dir,
		CommandMessage::Configured::<'_, &str>(CommandMessageConfiguration {
			secrets_to_hide: None,
			are_errors_silenced: true,
		}),
	)
	.await
	.is_ok()
	{
		run_cmd(
			"git",
			&["remote", "remove", contributor],
			&repo_dir,
			CommandMessage::Configured::<'_, &str>(
				CommandMessageConfiguration {
					secrets_to_hide: None,
					are_errors_silenced: false,
				},
			),
		)
		.await?;
	}

	let contributor_remote_branch =
		format!("{}/{}", contributor, contributor_branch);
	let token = gh_client.auth_token().await?;
	let secrets_to_hide = [token.as_str()];
	let secrets_to_hide = Some(&secrets_to_hide[..]);
	let contributor_repository_domain =
		format!("github.com/{}/{}.git", contributor, contributor_repo);
	let contributor_remote_address = format!(
		"https://x-access-token:{}@{}",
		token, contributor_repository_domain
	);

	run_cmd(
		"git",
		&["remote", "add", contributor, &contributor_remote_address],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	run_cmd(
		"git",
		&["fetch", contributor, contributor_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// The contributor's branch might exist from a previous run (not expected for a fresh clone).
	// If so, delete it so that it can be recreated.
	// Before deleting the branch, it's first required to checkout to a detached SHA so that any
	// branch can be deleted without problems (e.g. the branch we're trying to deleted might be the
	// one that is currently active, and so deleting it would fail).
	let head_sha_output = run_cmd_with_output(
		"git",
		&["rev-parse", "HEAD"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	run_cmd(
		"git",
		&[
			"checkout",
			String::from_utf8(head_sha_output.stdout)
				.context(Utf8)?
				.trim(),
		],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: true,
		}),
	)
	.await?;
	let _ = run_cmd(
		"git",
		&["branch", "-D", contributor_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: true,
		}),
	)
	.await;
	run_cmd(
		"git",
		&["checkout", "--track", &contributor_remote_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	let owner_remote = "origin";
	let owner_remote_branch = format!("{}/{}", owner_remote, owner_branch);

	let token = gh_client.auth_token().await?;
	let secrets_to_hide = [token.as_str()];
	let secrets_to_hide = Some(&secrets_to_hide[..]);
	let owner_repository_domain =
		format!("github.com/{}/{}.git", owner, owner_repo);
	let owner_remote_address = format!(
		"https://x-access-token:{}@{}",
		token, owner_repository_domain
	);
	run_cmd(
		"git",
		&["remote", "set-url", owner_remote, &owner_remote_address],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	run_cmd(
		"git",
		&["fetch", owner_remote, owner_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// Create master merge commit before updating packages
	run_cmd(
		"git",
		&["merge", &owner_remote_branch, "--no-ff", "--no-edit"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	Ok(SetupContributorBranchData {
		contributor_remote: contributor.into(),
		repo_dir: repo_dir_str.into(),
		contributor_remote_branch,
		secrets_to_hide: secrets_to_hide.map(|secrets_to_hide| {
			secrets_to_hide.iter().map(|str| str.to_string()).collect()
		}),
	})
}

pub enum RebaseOutcome {
	UpToDate,
	Pushed,
}
pub async fn rebase(
	state: &AppState,
	owner: &str,
	owner_repo: &str,
	owner_branch: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
) -> Result<RebaseOutcome> {
	let SetupContributorBranchData {
		contributor_remote,
		repo_dir,
		secrets_to_hide,
		..
	} = &setup_contributor_branch(
		state,
		owner,
		owner_repo,
		owner_branch,
		contributor,
		contributor_repo,
		contributor_branch,
	)
	.await?;
	let secrets_to_hide = secrets_to_hide.as_ref().map(|vec| &vec[..]);

	let push_output = run_cmd_with_output(
		"git",
		&[
			"push",
			"--porcelain",
			contributor_remote,
			contributor_branch,
		],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	let push_output = String::from_utf8(push_output.stdout).context(Utf8)?;
	let push_output = push_output.trim();
	log::info!("rebase push_output: {:?}", push_output);

	for line in push_output.lines() {
		if line.ends_with("[up to date]") {
			return Ok(RebaseOutcome::UpToDate);
		}
	}

	Ok(RebaseOutcome::Pushed)
}
