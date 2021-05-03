use async_recursion::async_recursion;
use regex::RegexBuilder;
use rocksdb::DB;
use snafu::ResultExt;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::{
	cmd::*, config::BotConfig, constants::*, error::*, github::*,
	github_bot::GithubBot, results, webhook::merge_or_wait, Result,
	COMPANION_LONG_REGEX, COMPANION_PREFIX_REGEX, COMPANION_SHORT_REGEX,
	PR_HTML_URL_REGEX,
};

async fn update_companion_repository(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
) -> Result<String> {
	let token = github_bot.client.auth_key().await?;
	let secrets_to_hide = [token.as_str()];
	let secrets_to_hide = Some(&secrets_to_hide[..]);

	let owner_repository_domain =
		format!("github.com/{}/{}.git", owner, owner_repo);
	let owner_remote_address = format!(
		"https://x-access-token:{}@{}",
		token, owner_repository_domain
	);
	let repo_dir = format!("./{}", owner_repo);

	if Path::new(&repo_dir).exists() {
		log::info!("{} is already cloned; skipping", owner_repository_domain);
	} else {
		run_cmd_in_cwd(
			"git",
			&["clone", "-v", &owner_remote_address],
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
	}

	let contributor_repository_domain =
		format!("github.com/{}/{}.git", contributor, contributor_repo);
	let contributor_remote_branch =
		format!("{}/{}", contributor, contributor_branch);
	let contributor_remote_address = format!(
		"https://x-access-token:{}@{}",
		token, contributor_repository_domain
	);

	// The contributor's remote entry might exist from a previous run (not expected for a fresh
	// clone). If so, delete it so that it can be recreated.
	if run_cmd(
		"git",
		&["remote", "get-url", contributor],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
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

	// Record the sha before performing any code updates
	let sha_before_update_output = run_cmd_with_output(
		"git",
		&["rev-parse", "HEAD"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	let sha_before_update =
		String::from_utf8_lossy(&sha_before_update_output.stdout[..]);
	let sha_before_update = sha_before_update.trim();

	let owner_remote = "origin";
	let owner_branch = "master";
	let owner_remote_branch = format!("{}/{}", owner_remote, owner_branch);

	run_cmd(
		"git",
		&["fetch", owner_remote, &owner_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// Create master merge commit before updating packages
	let master_merge_result = run_cmd(
		"git",
		&["merge", &owner_remote_branch, "--no-ff", "--no-edit"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await;
	if let Err(e) = master_merge_result {
		log::info!("Aborting companion update due to master merge failure");
		run_cmd(
			"git",
			&["merge", "--abort"],
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
		return Err(e);
	}

	// `cargo update` should normally make changes to the lockfile with the latest SHAs from Github
	run_cmd(
		"cargo",
		&[
			"update",
			"-vp",
			if owner_repo == "companion-for-processbot-staging" {
				"main-for-processbot-staging"
			} else {
				"sp-io"
			},
		],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	run_cmd(
		"git",
		&["commit", "-am", "update Substrate"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	// Check if any files have changed by the previous commands; if so, push the changes
	let changed_files_output = run_cmd_with_output(
		"git",
		&["diff", "--name-only", sha_before_update],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	let changed_files =
		String::from_utf8_lossy(&changed_files_output.stdout[..]);
	let changed_files = changed_files.trim().split('\n').collect::<Vec<&str>>();
	log::info!("Changed files: {:?}", changed_files);

	if changed_files.is_empty() {
		run_cmd(
			"git",
			&["reset", "--hard", sha_before_update],
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;

		Ok(sha_before_update.to_string())
	} else {
		// Push the changes through the Github API so that commits are verified

		let created_tree: CreatedTree = github_bot
			.client
			.post(
				&format!(
					"{}/repos/{}/{}/git/trees",
					crate::github_bot::GithubBot::BASE_URL,
					owner,
					owner_repo,
				),
				&serde_json::json!({
					"base_tree": sha_before_update,
					"tree": changed_files
						.iter()
						.map(|path| {
							let full_path = format!("{}/{}", &repo_dir, path);
							Ok(TreeObject {
								path,
								content: fs::read_to_string(&full_path)?,
								mode: format!(
									"{:o}",
									fs::metadata(&full_path)?
										.permissions()
										.mode()
								),
							})
						})
						.collect::<Result<Vec<TreeObject>, io::Error>>()
						.context(StdIO)?
				}),
			)
			.await?;

		let created_commit: CreatedCommit = github_bot
			.client
			.post(
				&format!(
					"{}/repos/{}/{}/git/commits",
					crate::github_bot::GithubBot::BASE_URL,
					owner,
					owner_repo,
				),
				&serde_json::json!({
					"message": "merge master branch and update Substrate",
					"tree": created_tree.sha,
					"parents": vec![sha_before_update],
				}),
			)
			.await?;

		// Github offers no way to update the head commit a fork's PR
		// See https://github.community/t/how-to-update-forks-pull-request-head-from-the-github-api/177649
		// Therefore
		// - Create a ref with the validated commit
		// - Pull it and push it into the PR
		// - Delete the ref after

		let ref_name = format!("refs/heads/processbot-{}", chrono::Utc::now());
		let ref_name = ref_name
			.replace(' ', "-")
			.replace('.', "-")
			.replace(':', "-");

		let _: CreatedRef = github_bot
			.client
			.post(
				&format!(
					"{}/repos/{}/{}/git/refs",
					crate::github_bot::GithubBot::BASE_URL,
					owner,
					owner_repo,
				),
				&serde_json::json!({
					"ref": &ref_name,
					"sha": &created_commit.sha
				}),
			)
			.await?;

		let repo_cmds = results!(
			run_cmd(
				"git",
				&["reset", "--hard", sha_before_update],
				&repo_dir,
				CommandMessage::Configured(CommandMessageConfiguration {
					secrets_to_hide,
					are_errors_silenced: false,
				}),
			)
			.await,
			run_cmd(
				"git",
				&["pull", "--ff-only", "origin", &ref_name],
				&repo_dir,
				CommandMessage::Configured(CommandMessageConfiguration {
					secrets_to_hide,
					are_errors_silenced: false,
				}),
			)
			.await,
			run_cmd_with_output(
				"git",
				&["rev-parse", "HEAD"],
				&repo_dir,
				CommandMessage::Configured(CommandMessageConfiguration {
					secrets_to_hide,
					are_errors_silenced: false,
				}),
			)
			.await,
			run_cmd(
				"git",
				&["push", contributor, contributor_branch],
				&repo_dir,
				CommandMessage::Configured(CommandMessageConfiguration {
					secrets_to_hide,
					are_errors_silenced: false,
				}),
			)
			.await
		);

		let _: Result<()> = github_bot
			.client
			.delete(
				&format!(
					"{}/repos/{}/{}/git/{}",
					crate::github_bot::GithubBot::BASE_URL,
					owner,
					owner_repo,
					&ref_name
				),
				&serde_json::json!({}),
			)
			.await;

		repo_cmds.map(|(_, _, sha_output, _)| {
			String::from_utf8_lossy(&sha_output.stdout[..])
				.trim()
				.to_string()
		})
	}
}

fn companion_parse(body: &str) -> Option<(String, String, String, i64)> {
	companion_parse_long(body).or(companion_parse_short(body))
}

fn companion_parse_long(body: &str) -> Option<(String, String, String, i64)> {
	let re = RegexBuilder::new(COMPANION_LONG_REGEX!())
		.case_insensitive(true)
		.build()
		.unwrap();
	let caps = re.captures(&body)?;
	let html_url = caps.name("html_url")?.as_str().to_owned();
	let owner = caps.name("owner")?.as_str().to_owned();
	let repo = caps.name("repo")?.as_str().to_owned();
	let number = caps
		.name("number")?
		.as_str()
		.to_owned()
		.parse::<i64>()
		.ok()?;
	Some((html_url, owner, repo, number))
}

fn companion_parse_short(body: &str) -> Option<(String, String, String, i64)> {
	let re = RegexBuilder::new(COMPANION_SHORT_REGEX!())
		.case_insensitive(true)
		.build()
		.unwrap();
	let caps = re.captures(&body)?;
	let owner = caps.name("owner")?.as_str().to_owned();
	let repo = caps.name("repo")?.as_str().to_owned();
	let number = caps
		.name("number")?
		.as_str()
		.to_owned()
		.parse::<i64>()
		.ok()?;
	let html_url = format!(
		"https://github.com/{owner}/{repo}/pull/{number}",
		owner = owner,
		repo = repo,
		number = number
	);
	Some((html_url, owner, repo, number))
}

#[async_recursion]
async fn perform_companion_update(
	github_bot: &GithubBot,
	db: &DB,
	html_url: &str,
	owner: &str,
	repo: &str,
	number: i64,
	bot_config: &BotConfig,
) -> Result<()> {
	let comp_pr = github_bot.pull_request(&owner, &repo, number).await?;

	if let PullRequest {
		head:
			Some(Head {
				ref_field: Some(contributor_branch),
				repo:
					Some(HeadRepo {
						name: contributor_repo,
						owner:
							Some(User {
								login: contributor, ..
							}),
						..
					}),
				..
			}),
		..
	} = comp_pr.clone()
	{
		log::info!("Updating companion {}", html_url);
		let updated_sha = update_companion_repository(
			github_bot,
			&owner,
			&repo,
			&contributor,
			&contributor_repo,
			&contributor_branch,
		)
		.await?;

		log::info!("Companion updated; waiting for checks on {}", html_url);
		merge_or_wait(
			MergeWaitMode::CanWait,
			github_bot,
			owner,
			repo,
			&comp_pr,
			bot_config,
			&format!("parity-processbot[bot]"),
			db,
			&updated_sha,
		)
		.await?;
	} else {
		return Err(Error::Message {
			msg: "Companion PR is missing some API data.".to_string(),
		});
	}

	Ok(())
}

async fn detect_then_update_companion(
	github_bot: &GithubBot,
	merge_done_in: &str,
	pr: &PullRequest,
	db: &DB,
	bot_config: &BotConfig,
) -> Result<()> {
	if merge_done_in == "substrate"
		|| merge_done_in == "main-for-processbot-staging"
	{
		log::info!("Checking for companion.");
		if let Some((html_url, owner, repo, number)) =
			pr.body.as_ref().map(|body| companion_parse(body)).flatten()
		{
			log::info!("Found companion {}", html_url);
			perform_companion_update(
				github_bot, db, &html_url, &owner, &repo, number, bot_config,
			)
			.await
			.map_err(|e| e.map_issue((owner, repo, number)))?;
		} else {
			log::info!("No companion found.");
		}
	}

	Ok(())
}

/// Check for a Polkadot companion and update it if found.
pub async fn update_companion(
	github_bot: &GithubBot,
	merge_done_in: &str,
	pr: &PullRequest,
	db: &DB,
	bot_config: &BotConfig,
) -> Result<()> {
	detect_then_update_companion(github_bot, merge_done_in, pr, db, bot_config)
		.await
		.map_err(|e| match e {
			Error::WithIssue { source, issue } => {
				Error::CompanionUpdate { source }.map_issue(issue)
			}
			_ => {
				let e = Error::CompanionUpdate {
					source: Box::new(e),
				};
				if let Some(details) = pr.get_issue_details() {
					e.map_issue(details)
				} else {
					e
				}
			}
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_companion_parse() {
		// Extra params should not be included in the parsed URL
		assert_eq!(
			companion_parse(
				"companion: https://github.com/paritytech/polkadot/pull/1234?extra_params=true"
			),
			Some((
				"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
				"paritytech".to_owned(),
				"polkadot".to_owned(),
				1234
			))
		);

		// Should be case-insensitive on the "companion" marker
		for companion_marker in &["Companion", "companion"] {
			// Long version should work even if the body has some other content around the
			// companion text
			assert_eq!(
				companion_parse(&format!(
					"
					Companion line is in the middle
					{}: https://github.com/paritytech/polkadot/pull/1234
					Final line
					",
					companion_marker
				)),
				Some((
					"https://github.com/paritytech/polkadot/pull/1234"
						.to_owned(),
					"paritytech".to_owned(),
					"polkadot".to_owned(),
					1234
				))
			);

			// Short version should work even if the body has some other content around the
			// companion text
			assert_eq!(
				companion_parse(&format!(
					"
					Companion line is in the middle
					{}: paritytech/polkadot#1234
			        Final line
					",
					companion_marker
				)),
				Some((
					"https://github.com/paritytech/polkadot/pull/1234"
						.to_owned(),
					"paritytech".to_owned(),
					"polkadot".to_owned(),
					1234
				))
			);
		}

		// Long version should not be detected if "companion: " and the expression are not both in
		// the same line
		assert_eq!(
			companion_parse(
				"
				I want to talk about companion: but NOT reference it
				I submitted it in https://github.com/paritytech/polkadot/pull/1234
				"
			),
			None
		);

		// Short version should not be detected if "companion: " and the expression are not both in
		// the same line
		assert_eq!(
			companion_parse(
				"
				I want to talk about companion: but NOT reference it
				I submitted it in paritytech/polkadot#1234
				"
			),
			None
		);
	}
}
