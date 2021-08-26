use super::*;
use crate::{types::*, utils::*};

async fn detect_then_update_companion(
	&self,
	pr: &PullRequest,
	db: &DB,
	merge_done_in: &str,
) -> Result<()> {
	if merge_done_in == "substrate" || merge_done_in == MAIN_REPO_FOR_STAGING {
		log::info!("Checking for companion.");

		if let Some(IssueDetailsWithRepositoryURL {
			issue: IssueDetails {
				owner,
				repo,
				number,
			},
			html_url,
		}) = pr.body.as_ref().map(parse_companion_description).flatten()
		{
			log::info!("Found companion {}", html_url);
			perform_companion_update(
				self,
				db,
				PerformCompanionUpdateArgs {
					html_url,
					owner,
					repo,
					number,
					merge_done_in,
				},
			)
			.await
			.map_err(|e| e.map_issue((owner, repo, number)))?;
		} else {
			log::info!("No companion found.");
		}
	}

	Ok(())
}

impl Bot {
	async fn update_companion_repository<'a>(
		&self,
		args: UpdateCompanionRepositoryArgs<'a>,
	) -> Result<String> {
		let UpdateCompanionRepositoryArgs {
			owner,
			owner_repo,
			contributor,
			contributor_repo,
			contributor_branch,
			merge_done_in,
		} = args;
		let token = self.client.auth_key().await?;
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
			log::info!(
				"{} is already cloned; skipping",
				owner_repository_domain
			);
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

		// `cargo update` should normally make changes to the lockfile with the latest SHAs from Github
		run_cmd(
			"cargo",
			&[
				"update",
				"-vp",
				if merge_done_in == MAIN_REPO_FOR_STAGING {
					MAIN_REPO_FOR_STAGING
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

		// Check if `cargo update` resulted in any changes. If the master merge commit already had the
		// latest lockfile then no changes might have been made.
		let changes_after_update_output = run_cmd_with_output(
			"git",
			&["status", "--short"],
			&repo_dir,
			CommandMessage::Configured(CommandMessageConfiguration {
				secrets_to_hide,
				are_errors_silenced: false,
			}),
		)
		.await?;
		if !String::from_utf8_lossy(&(&changes_after_update_output).stdout[..])
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

	async fn perform_companion_update<'a>(
		&self,
		db: &DB,
		args: PerformCompanionUpdateArgs<'a>,
	) -> Result<()> {
		let PerformCompanionUpdateArgs {
			html_url,
			owner,
			repo,
			number,
			merge_done_in,
		} = args;
		let comp_pr = self.pull_request(&owner, &repo, number).await?;

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
				self,
				UpdateCompanionRepositoryArgs {
					owner,
					repo,
					contributor,
					contributor_repo,
					contributor_branch,
					merge_done_in,
				},
			)
			.await?;

			log::info!("Companion updated; waiting for checks on {}", html_url);
			self.wait_to_merge(
				&owner,
				&repo,
				comp_pr.number,
				&comp_pr.html_url,
				&format!("parity-processbot[bot]"),
				&updated_sha,
				db,
			)
			.await?;
		} else {
			return Err(Error::Message {
				msg: "Companion PR is missing some API data.".to_string(),
			});
		}

		Ok(())
	}

	pub async fn update_companion(
		&self,
		merge_done_in: &str,
		pr: &PullRequest,
		db: &DB,
	) -> Result<()> {
		detect_then_update_companion(self, merge_done_in, pr, db)
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
}
