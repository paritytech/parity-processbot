use async_recursion::async_recursion;
use regex::RegexBuilder;
use snafu::ResultExt;
use std::{
	collections::HashMap, collections::HashSet, iter::FromIterator, path::Path,
	time::Duration,
};
use tokio::time::delay_for;

use crate::{
	cmd::*,
	error::*,
	github::*,
	github_bot::GithubBot,
	webhook::{
		check_merge_is_allowed, cleanup_merged_pr, get_latest_checks_state,
		get_latest_statuses_state, merge, ready_to_merge, wait_to_merge,
		AppState, MergeRequest,
	},
	MergeAllowedOutcome, Result, Status, COMPANION_LONG_REGEX,
	COMPANION_PREFIX_REGEX, COMPANION_SHORT_REGEX, PR_HTML_URL_REGEX,
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
			&String::from_utf8(head_sha_output.stdout)
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
	log::info!(
		"Updating references of {} in the Cargo.lock of {}",
		url_merge_was_done_in,
		repo_dir
	);
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

	// Check if `cargo update` resulted in any changes. If the master merge commit already had an
	// up-to-date lockfile then no changes might have been made.
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
	if !String::from_utf8_lossy(&output.stdout[..])
		.trim()
		.is_empty()
	{
		run_cmd(
			"git",
			&[
				"commit",
				"-am",
				&format!("update lockfile for {}", merge_done_in),
			],
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

pub fn parse_all_companions(
	companion_reference_trail: &Vec<(String, String)>,
	body: &str,
) -> Vec<IssueDetailsWithRepositoryURL> {
	body.lines()
		.filter_map(|line| {
			companion_parse(line)
				.map(|comp| {
					// Break cyclical references between dependency and dependents because we're only
					// interested in the dependency -> dependent relationship, not the other way around.
					for (owner, repo) in companion_reference_trail {
						if &comp.1 == owner && &comp.2 == repo {
							return None;
						}
					}
					Some(comp)
				})
				.flatten()
		})
		.collect()
}

#[async_recursion]
pub async fn check_all_companions_are_mergeable(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
	companion_reference_trail: &Vec<(String, String)>,
) -> Result<()> {
	let companions = match pr.parse_all_companions(companion_reference_trail) {
		Some(companions) => {
			if companions.is_empty() {
				return Ok(());
			} else {
				companions
			}
		}
		_ => return Ok(()),
	};

	let AppState { github_bot, .. } = state;
	for (html_url, owner, repo, number) in companions {
		let companion = github_bot.pull_request(&owner, &repo, number).await?;

		if companion.merged {
			continue;
		}

		let has_user_owner = companion
			.user
			.as_ref()
			.map(|user| {
				user.type_field
					.as_ref()
					.map(|user_type| user_type == &UserType::User)
					.unwrap_or(false)
			})
			.unwrap_or(false);
		if !has_user_owner {
			return Err(Error::Message {
				msg: format!(
					"Companion {} is not owned by a user, therefore processbot would not be able to push the lockfile update to their branch due to a Github limitation (https://github.com/isaacs/github/issues/1681)",
					html_url
				),
			});
		}

		if !companion.maintainer_can_modify
			// Even if the "Allow edits from maintainers" setting is not enabled, as long as the
			// companion belongs to the same organization, the bot should still be able to push
			// commits.
			&& companion
				.head
				.repo
				.owner.login != pr.base.repo.owner.login
		{
			return Err(Error::Message {
				msg: format!(
					"Github API says \"Allow edits from maintainers\" is not enabled for {}. The bot would use that permission to push the lockfile update after merging this PR. Please check https://docs.github.com/en/github/collaborating-with-pull-requests/working-with-forks/allowing-changes-to-a-pull-request-branch-created-from-a-fork.",
					html_url
				),
			});
		}

		// Keeping track of the trail of references is necessary to break chains like A -> B -> C -> A
		// TODO: of course this should be tested
		let next_companion_reference_trail = {
			let mut next_trail: Vec<(String, String)> =
				Vec::with_capacity(companion_reference_trail.len() + 1);
			next_trail.extend_from_slice(&companion_reference_trail[..]);
			next_trail.push((
				pr.base.repo.owner.login.to_owned(),
				pr.base.repo.name.to_owned(),
			));
			next_trail
		};
		match check_merge_is_allowed(
			state,
			&companion,
			&requested_by,
			None,
			&next_companion_reference_trail,
		)
		.await?
		{
			MergeAllowedOutcome::Disallowed(msg) => {
				return Err(Error::Message { msg })
			}
			_ => (),
		}

		match get_latest_statuses_state(
			github_bot,
			&companion.base.repo.owner.login,
			&companion.base.repo.name,
			&companion.head.sha,
			&companion.html_url,
		)
		.await?
		{
			Status::Success => (),
			Status::Pending => {
				return Err(Error::InvalidCompanionStatus {
					value: InvalidCompanionStatusValue::Pending,
					msg: format!(
						"Companion {} has pending statuses",
						companion.html_url
					),
				});
			}
			Status::Failure => {
				return Err(Error::InvalidCompanionStatus {
					value: InvalidCompanionStatusValue::Failure,
					msg: format!(
						"Companion {} has failed statuses",
						companion.html_url
					),
				});
			}
		};

		match get_latest_checks_state(
			github_bot,
			&companion.base.repo.owner.login,
			&companion.base.repo.name,
			&companion.head.sha,
			&companion.html_url,
		)
		.await?
		{
			Status::Success => (),
			Status::Pending => {
				return Err(Error::InvalidCompanionStatus {
					value: InvalidCompanionStatusValue::Pending,
					msg: format!(
						"Companion {} has pending checks",
						companion.html_url
					),
				});
			}
			Status::Failure => {
				return Err(Error::InvalidCompanionStatus {
					value: InvalidCompanionStatusValue::Failure,
					msg: format!(
						"Companion {} has failed checks",
						companion.html_url
					),
				});
			}
		};
	}

	Ok(())
}

#[async_recursion]
async fn update_then_merge_companion(
	state: &AppState,
	owner: &str,
	repo: &str,
	number: &i64,
	html_url: &str,
	merge_done_in: &str,
	requested_by: &str,
) -> Result<()> {
	let AppState { github_bot, .. } = state;

	let companion = github_bot.pull_request(&owner, &repo, *number).await?;
	if cleanup_merged_pr(state, &companion)? {
		return Ok(());
	}

	if let Err(err) =
		check_merge_is_allowed(state, &companion, requested_by, None, &vec![])
			.await
	{
		match err {
			Error::InvalidCompanionStatus { ref value, .. } => match value {
				InvalidCompanionStatusValue::Pending => (),
				InvalidCompanionStatusValue::Failure => return Err(err),
			},
			err => return Err(err),
		};
	}

	log::info!("Updating companion {}", html_url);
	let updated_sha = update_companion_repository(
		github_bot,
		owner,
		repo,
		&companion.head.repo.owner.login,
		&companion.head.repo.name,
		&companion.head.ref_field,
		merge_done_in,
	)
	.await?;

	// Wait a bit for the statuses to settle after we've updated the companion
	delay_for(Duration::from_millis(4096)).await;

	// Fetch it again since we've pushed some commits and therefore some status or check might have
	// failed already
	let companion = github_bot.pull_request(&owner, &repo, *number).await?;

	let should_wait_for_companions = match check_merge_is_allowed(
		state,
		&companion,
		requested_by,
		None,
		&vec![],
	)
	.await
	{
		Ok(_) => false,
		Err(err) => match err {
			Error::InvalidCompanionStatus { ref value, .. } => match value {
				InvalidCompanionStatusValue::Pending => true,
				InvalidCompanionStatusValue::Failure => return Err(err),
			},
			err => return Err(err),
		},
	};

	if !should_wait_for_companions
		&& ready_to_merge(&state.github_bot, &companion).await?
	{
		if let Err(err) = merge(state, &companion, requested_by, None).await? {
			return match err {
				Error::MergeFailureWillBeSolvedLater { .. } => Ok(()),
				err => Err(err),
			};
		}
		if let Err(err) =
			merge_companions(state, &companion, &requested_by, None).await
		{
			log::error!(
				"Failed to merge companions of {} (a companion) due to {:?}",
				companion.html_url,
				err
			);
		}
	} else {
		log::info!("Companion updated; waiting for checks on {}", html_url);

		let companion_children = companion.parse_all_mr_base(&vec![]);

		let msg = if let Some(true) = companion_children
			.as_ref()
			.map(|children| !children.is_empty())
		{
			Some("Waiting for companions' statuses and this PR's statuses")
		} else {
			None
		};

		wait_to_merge(
			state,
			&updated_sha,
			&MergeRequest {
				owner: companion.base.repo.owner.login,
				repo: companion.base.repo.name,
				number: companion.number,
				html_url: companion.html_url,
				requested_by: requested_by.to_owned(),
				companion_children: companion_children,
			},
			msg,
		)
		.await?;
	}

	Ok(())
}

pub async fn merge_companions(
	state: &AppState,
	pr: &PullRequest,
	requested_by: &str,
	prevent_error_post_of: Option<&str>,
) -> Result<()> {
	log::info!("Checking for companions in {}", pr.html_url);

	let companions_groups = {
		let companions = match pr.parse_all_companions(&vec![]) {
			Some(companions) => {
				if companions.is_empty() {
					return Ok(());
				} else {
					companions
				}
			}
			None => return Ok(()),
		};

		let mut companions_groups: HashMap<
			String,
			Vec<IssueDetailsWithRepositoryURL>,
		> = HashMap::new();
		for comp in companions {
			let (_, ref owner, ref repo, _) = comp;
			let key = format!("{}/{}", owner, repo);
			if let Some(group) = companions_groups.get_mut(&key) {
				group.push(comp);
			} else {
				companions_groups.insert(key, vec![comp]);
			}
		}

		companions_groups
	};

	let AppState { github_bot, .. } = state;

	let mut remaining_futures = companions_groups
		.into_values()
		.map(|group| {
			Box::pin(async move {
				let mut errors: Vec<CompanionDetailsWithErrorMessage> = vec![];

				for (html_url, owner, repo, ref number) in group {
					if let Err(err) = update_then_merge_companion(
						state,
						&owner,
						&repo,
						number,
						&html_url,
						&pr.base.repo.name,
						requested_by,
					)
					.await
					{
						errors.push(CompanionDetailsWithErrorMessage {
							owner: owner.to_owned(),
							repo: repo.to_owned(),
							number: *number,
							html_url: html_url.to_owned(),
							msg: format!("Companion update failed: {:?}", err),
						});
					}
				}

				errors
			})
		})
		.collect::<Vec<_>>();

	let mut errors: Vec<CompanionDetailsWithErrorMessage> = vec![];
	while !remaining_futures.is_empty() {
		let (result, _, next_remaining_futures) =
			futures::future::select_all(remaining_futures).await;
		remaining_futures = next_remaining_futures;

		for error in result {
			let CompanionDetailsWithErrorMessage {
				ref owner,
				ref repo,
				ref number,
				ref html_url,
				ref msg,
			} = error;
			if prevent_error_post_of != Some(html_url) {
				let _ = github_bot
					.create_issue_comment(owner, repo, *number, msg)
					.await
					.map_err(|e| {
						log::error!("Error posting comment: {}", e);
					});
			}
			errors.push(error);
		}
	}

	if errors.is_empty() {
		Ok(())
	} else {
		Err(Error::CompanionsFailedMerge { errors })
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
		let owner = "paritytech";
		let repo = "polkadot";
		let pr_number = 1234;
		let companion_url =
			format!("https://github.com/{}/{}/pull/{}", owner, repo, pr_number);
		let expected_companion = (
			companion_url.to_owned(),
			owner.to_owned(),
			repo.to_owned(),
			pr_number,
		);
		assert_eq!(
			parse_all_companions(
				&vec![],
				&format!(
					"
					first companion: {}
					second companion: {}
				",
					&companion_url, &companion_url
				)
			),
			vec![expected_companion.clone(), expected_companion.clone()]
		);
	}

	#[test]
	fn test_cyclical_references() {
		let owner = "paritytech";
		let repo = "polkadot";
		let companion_description = format!(
			"
				{} companion: https://github.com/{}/{}/pull/123
				",
			owner, owner, repo,
		);

		// If the source is not referenced in the description, something is parsed
		assert_ne!(
			parse_all_companions(&vec![], &companion_description),
			vec![]
		);

		// If the source is referenced in the description, it is omitted
		assert_eq!(
			parse_all_companions(
				&vec![(owner.to_owned(), repo.to_owned())],
				&companion_description
			),
			vec![]
		);
	}
}
