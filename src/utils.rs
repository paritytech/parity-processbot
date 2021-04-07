use crate::{cmd::*, error::*, github_bot::GithubBot, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use snafu::ResultExt;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct RepositoryUpdateOutput {
	pub base_sha: String,
	pub head_sha: String,
}

pub enum RepositoryUpdateStrategy {
	FromSubstrateToPolkadotCompanion,
}

static PREV_TEMP_BRANCHES: OnceCell<Mutex<HashSet<String>>> = OnceCell::new();
fn get_unique_branch_name(branch: &str) -> String {
	let mut prev_branches = PREV_TEMP_BRANCHES
		.get_or_init(|| Mutex::new(HashSet::new()))
		.lock();
	let mut branch = branch.to_string();
	let unique_branch_name = loop {
		branch.push_str("_tmp");
		if prev_branches.insert(branch.clone()) {
			break branch;
		}
	};
	unique_branch_name
}

pub static REPOSITORIES_DIR: OnceCell<PathBuf> = OnceCell::new();
pub async fn update_repository(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
	update_strategy: Option<RepositoryUpdateStrategy>,
) -> Result<RepositoryUpdateOutput> {
	let token = github_bot.client.auth_key().await?;
	let secrets_to_hide = vec![token.clone()];
	let (repositories_dir, repo_dir, secrets_to_hide, dirs_to_hide) = {
		if let Some(repositories_dir) = REPOSITORIES_DIR.get() {
			let repo_path = repositories_dir.join(owner_repo);
			let repo_dir = repo_path.to_str().unwrap();
			(
				repositories_dir.clone().to_str().unwrap().to_string(),
				repo_dir.to_string(),
				Some([&secrets_to_hide[..], &[repo_dir.to_string()]].concat()),
				Some(vec![repositories_dir
					.clone()
					.to_str()
					.unwrap()
					.to_string()]),
			)
		} else {
			(
				".".to_string(),
				format!("./{}", owner_repo),
				Some(secrets_to_hide),
				None,
			)
		}
	};
	let secrets_to_hide = secrets_to_hide.as_ref();
	let dirs_to_hide = dirs_to_hide.as_ref();

	let (owner_remote_address, owner_repository_domain) =
		github_bot.get_fetch_components(owner, owner_repo, &token);

	if Path::new(&repo_dir).exists() {
		log::info!("{} is already cloned; skipping", &owner_repository_domain);
	} else {
		run_cmd(
			"git",
			&["clone", "-v", &owner_remote_address],
			repositories_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				dirs_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
	}

	let (contributor_remote_address, _) =
		github_bot.get_fetch_components(contributor, contributor_repo, &token);
	let contributor_remote_branch =
		format!("{}/{}", contributor, contributor_branch);

	// The contributor's remote entry might exist from a previous run (not expected for a fresh
	// clone). If so, delete it so that it can be recreated.
	if run_cmd(
		"git",
		&["remote", "get-url", contributor],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
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
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				dirs_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
	}
	run_cmd(
		"git",
		&["remote", "add", contributor, &contributor_remote_address],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
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
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// The contributor's branch might exist from a previous run (not expected for a fresh clone).
	// If so, delete it so that it can be recreated.
	// First switch to a temporary branch so that the contributor's branch can be deleted
	// regardless of previous errors in this directory (Git does not allow deletion of the
	// currently-checked-out branch, which might be the case).
	let temp_branch_name = get_unique_branch_name(contributor_branch);
	run_cmd(
		"git",
		&["checkout", "-b", &temp_branch_name],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	// Now delete and recreate the contributor's branch
	let _ = run_cmd(
		"git",
		&["branch", "-D", contributor_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
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
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	// Then clean up the temporary branch
	run_cmd(
		"git",
		&["branch", "-D", &temp_branch_name],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// Ensure the owner branch has the latest remote changes so that the merge commit is always
	// done with the most updated code from Github.
	let owner_remote = "origin";
	let owner_branch = "master";
	let owner_remote_branch = format!("{}/{}", owner_remote, owner_branch);

	run_cmd(
		"git",
		&["fetch", owner_remote, &owner_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// Keep track of the HEAD for the owner's branch *after* fetching the latest changes; in case
	// this reference does not change between attempts, then the master merge commit will have the
	// same effect as the previous attempt, resulting in failure again, thus it's not meaningful to
	// keep retrying in that case.
	let base_sha = {
		let output = run_cmd_with_output(
			"git",
			&["rev-parse", &owner_remote_branch],
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				dirs_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
		String::from_utf8(output.stdout)
			.context(Utf8)?
			.trim()
			.to_string()
	};

	// Create master merge commit before updating packages
	let master_merge_result = run_cmd(
		"git",
		&["merge", &owner_remote_branch, "--no-ff", "--no-edit"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await;
	if let Err(e) = master_merge_result {
		log::info!("Aborting repository update due to master merge failure");
		let _ = run_cmd(
			"git",
			&["merge", "--abort"],
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				dirs_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await;
		return Err(e);
	}

	match update_strategy {
		Some(RepositoryUpdateStrategy::FromSubstrateToPolkadotCompanion) => {
			// `cargo update` should normally make changes to the lockfile with the latest SHAs
			// from Github
			run_cmd(
				"cargo",
				&["update", "-vp", "sp-io"],
				&repo_dir,
				CommandMessage::Configured(CommandMessageConfiguration {
					secrets_to_hide,
					dirs_to_hide,
					are_errors_silenced: false,
				}),
			)
			.await?;

			// Check if `cargo update` resulted in any changes. If the master merge commit
			// already had the latest lockfile then no changes might have been made.
			let changes_after_update_output = run_cmd_with_output(
				"git",
				&["status", "--short"],
				&repo_dir,
				CommandMessage::Configured(CommandMessageConfiguration {
					secrets_to_hide,
					dirs_to_hide,
					are_errors_silenced: false,
				}),
			)
			.await?;
			if !String::from_utf8_lossy(
				&(&changes_after_update_output).stdout[..],
			)
			.trim()
			.is_empty()
			{
				run_cmd(
					"git",
					&["commit", "-am", "update Substrate"],
					&repo_dir,
					CommandMessage::Configured(CommandMessageConfiguration {
						secrets_to_hide,
						dirs_to_hide,
						are_errors_silenced: false,
					}),
				)
				.await?;
			}
		}
		None => (),
	};

	run_cmd(
		"git",
		&["push", contributor, contributor_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			dirs_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	log::info!(
		"Getting the head SHA after a repository update in {}",
		&contributor_remote_branch
	);
	let head_sha = {
		let output = run_cmd_with_output(
			"git",
			&["rev-parse", "HEAD"],
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				dirs_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
		String::from_utf8(output.stdout)
			.context(Utf8)?
			.trim()
			.to_string()
	};

	Ok(RepositoryUpdateOutput { head_sha, base_sha })
}

pub fn result_t2<T1, T2>(r1: Result<T1>, r2: Result<T2>) -> Result<(T1, T2)> {
	match r1 {
		Ok(r1) => match r2 {
			Ok(r2) => Ok((r1, r2)),
			Err(e) => Err(e),
		},
		Err(e) => Err(e),
	}
}
