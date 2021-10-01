use regex::RegexBuilder;
use rocksdb::DB;
use snafu::ResultExt;
use std::{
	collections::hash_set::HashSet, iter::FromIterator, path::Path,
	time::Duration,
};
use tokio::time::delay_for;

use crate::{
	cmd::*,
	config::BotConfig,
	constants::MAIN_REPO_FOR_STAGING,
	error::*,
	github::*,
	github_bot::GithubBot,
	webhook::{merge, ready_to_merge, wait_to_merge},
	Result, COMPANION_LONG_REGEX, COMPANION_PREFIX_REGEX,
	COMPANION_SHORT_REGEX, PR_HTML_URL_REGEX,
};

async fn update_companion_repository(
	github_bot: &GithubBot,
	owner: &str,
	owner_repo: &str,
	contributor: &str,
	contributor_repo: &str,
	contributor_branch: &str,
	merge_done_in: &str,
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

	// Update the packages from 'merge_done_in' in the companion
	let url_merge_was_done_in =
		format!("https://github.com/{}/{}", owner, merge_done_in);
	let cargo_lock_path = Path::new(&repo_dir).join("Cargo.lock");
	let lockfile =
		cargo_lock::Lockfile::load(cargo_lock_path).map_err(|err| {
			Error::Message {
				msg: format!(
					"Failed to parse lockfile of {}: {:?}",
					contributor_repo, err
				),
			}
		})?;
	let pkgs_in_companion: HashSet<&str> = {
		HashSet::from_iter(lockfile.packages.iter().filter_map(|pkg| {
			if let Some(src) = pkg.source.as_ref() {
				if src.url().as_str() == url_merge_was_done_in {
					Some(pkg.name.as_str())
				} else {
					None
				}
			} else {
				None
			}
		}))
	};
	if !pkgs_in_companion.is_empty() {
		let args = {
			let mut args = vec!["update", "-v"];
			args.extend(
				pkgs_in_companion.iter().map(|pkg| ["-p", pkg]).flatten(),
			);
			args
		};
		run_cmd(
			"cargo",
			&args,
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
	}

	// Check if `cargo update` resulted in any changes. If the master merge commit already had the
	// latest lockfile then no changes might have been made.
	let output = run_cmd_with_output(
		"git",
		&["status", "--short"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	if !String::from_utf8_lossy(&(&output).stdout[..])
		.trim()
		.is_empty()
	{
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
	}

	run_cmd(
		"git",
		&["push", contributor, contributor_branch],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;

	log::info!(
		"Getting the head SHA after a companion update in {}",
		&contributor_remote_branch
	);
	let updated_sha_output = run_cmd_with_output(
		"git",
		&["rev-parse", "HEAD"],
		&repo_dir,
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			are_errors_silenced: false,
		}),
	)
	.await?;
	let updated_sha = String::from_utf8(updated_sha_output.stdout)
		.context(Utf8)?
		.trim()
		.to_string();

	Ok(updated_sha)
}

fn companion_parse(body: &str) -> Option<IssueDetailsWithRepositoryURL> {
	companion_parse_long(body).or(companion_parse_short(body))
}

fn companion_parse_long(body: &str) -> Option<IssueDetailsWithRepositoryURL> {
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

fn companion_parse_short(body: &str) -> Option<IssueDetailsWithRepositoryURL> {
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

fn parse_all_companions(body: &str) -> Vec<IssueDetailsWithRepositoryURL> {
	body.lines().filter_map(companion_parse).collect()
}

pub async fn check_all_companions_are_mergeable(
	github_bot: &GithubBot,
	pr: &PullRequest,
	merge_done_in: &str,
) -> Result<()> {
	if merge_done_in == "substrate" || merge_done_in == MAIN_REPO_FOR_STAGING {
		if let Some(body) = pr.body.as_ref() {
			for (html_url, owner, repo, number) in parse_all_companions(body) {
				let companion =
					github_bot.pull_request(&owner, &repo, number).await?;

				let is_owner_a_user = companion
					.user
					.as_ref()
					.map(|user| {
						user.type_field
							.as_ref()
							.map(|user_type| user_type == &UserType::User)
							.unwrap_or(false)
					})
					.unwrap_or(false);
				if !is_owner_a_user {
					return Err(Error::Message {
						msg: format!(
							"Companion {} is not owned by a user, therefore we will not be able to push the lockfile update to their branch due to a Github limitation (https://github.com/isaacs/github/issues/1681)",
							html_url
						),
					});
				}

				let is_mergeable = companion
					.mergeable
					.map(|mergeable| mergeable)
					.unwrap_or(false);
				if !is_mergeable {
					return Err(Error::Message {
						msg: format!(
							"Github API says companion {} is not mergeable",
							html_url
						),
					});
				}
			}
		}
	}

	Ok(())
}

async fn update_then_merge_companion(
	github_bot: &GithubBot,
	bot_config: &BotConfig,
	db: &DB,
	html_url: &str,
	owner: &str,
	repo: &str,
	number: &i64,
	merge_done_in: &str,
) -> Result<()> {
	let pr = github_bot.pull_request(&owner, &repo, *number).await?;

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
	} = pr.clone()
	{
		log::info!("Updating companion {}", html_url);
		let updated_sha = update_companion_repository(
			github_bot,
			owner,
			repo,
			&contributor,
			&contributor_repo,
			&contributor_branch,
			merge_done_in,
		)
		.await?;

		// Wait a bit for all the statuses to settle after we've updated the companion.
		delay_for(Duration::from_millis(4096)).await;

		if ready_to_merge(github_bot, owner, repo, &pr).await? {
			merge(
				github_bot,
				owner,
				repo,
				&pr,
				bot_config,
				"parity-processbot[bot]",
				None,
			)
			.await??;
		} else {
			log::info!("Companion updated; waiting for checks on {}", html_url);
			wait_to_merge(
				github_bot,
				&owner,
				&repo,
				pr.number,
				&pr.html_url,
				&format!("parity-processbot[bot]"),
				&updated_sha,
				db,
			)
			.await?;
		}
	} else {
		return Err(Error::Message {
			msg: "Companion PR is missing some API data.".to_string(),
		});
	}

	Ok(())
}

pub async fn merge_companions(
	github_bot: &GithubBot,
	bot_config: &BotConfig,
	merge_done_in: &str,
	pr: &PullRequest,
	db: &DB,
) -> Result<()> {
	let mut errors: Vec<String> = vec![];

	if merge_done_in == "substrate" || merge_done_in == MAIN_REPO_FOR_STAGING {
		log::info!("Checking for companion.");

		if let Some(body) = pr.body.as_ref() {
			let companions = parse_all_companions(body);
			if companions.is_empty() {
				log::info!("No companion found.");
			} else {
				let mut remaining_futures = companions
					.iter()
					.map(|(html_url, owner, repo, ref number)| {
						Box::pin(async move {
							update_then_merge_companion(
								github_bot,
								bot_config,
								db,
								&html_url,
								&owner,
								&repo,
								number,
								merge_done_in,
							)
							.await
							.map_err(|err| CompanionDetailsWithErrorMessage {
								owner: owner.to_owned(),
								repo: repo.to_owned(),
								number: *number,
								html_url: html_url.to_owned(),
								msg: format!(
									"Companion update failed: {:?}",
									err
								),
							})
						})
					})
					.collect::<Vec<_>>();
				while !remaining_futures.is_empty() {
					let (result, _, next_remaining_futures) =
						futures::future::select_all(remaining_futures).await;
					if let Err(CompanionDetailsWithErrorMessage {
						ref owner,
						ref repo,
						ref number,
						ref html_url,
						ref msg,
					}) = result
					{
						let _ = github_bot
							.create_issue_comment(owner, repo, *number, msg)
							.await
							.map_err(|e| {
								log::error!("Error posting comment: {}", e);
							});
						errors.push(format!("{} {}", html_url, msg));
					}
					remaining_futures = next_remaining_futures;
				}
			}
		}
	}

	if errors.is_empty() {
		Ok(())
	} else {
		Err(Error::Message {
			msg: format!("Companion update failed: {}", errors.join("\n")),
		})
	}
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

	#[test]
	fn test_parse_all_companions() {
		let companion_url = "https://github.com/paritytech/polkadot/pull/1234";
		let expected_companion = (
			"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
			"paritytech".to_owned(),
			"polkadot".to_owned(),
			1234,
		);
		assert_eq!(
			parse_all_companions(&format!(
				"
					first companion: {}
					second companion: {}
				",
				companion_url, companion_url
			)),
			vec![expected_companion.clone(), expected_companion.clone()]
		);
	}
}
