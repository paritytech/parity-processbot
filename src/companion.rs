use regex::RegexBuilder;
use rocksdb::DB;

use crate::{
	error::*,
	github::*,
	github_bot::GithubBot,
	utils::*,
	webhook::{wait_to_merge, MergeRequest},
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
) -> Result<RepositoryUpdateOutput> {
	update_repository(
		github_bot,
		owner,
		owner_repo,
		contributor,
		contributor_repo,
		contributor_branch,
		Some(RepositoryUpdateStrategy::FromSubstrateToPolkadotCompanion),
	)
	.await
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

async fn perform_companion_update(
	github_bot: &GithubBot,
	db: &DB,
	contributor: &str,
	contributor_repo: &str,
	number: i64,
) -> Result<()> {
	let comp_pr = github_bot
		.pull_request(&contributor, &contributor_repo, number)
		.await?;

	if let PullRequest {
		number,
		html_url,
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
		base:
			Base {
				repo:
					Some(HeadRepo {
						name: owner_repo,
						owner: Some(User { login: owner, .. }),
					}),
				..
			},
		..
	} = comp_pr
	{
		log::info!("Updating companion {}", &html_url);
		let companion_update_result = update_companion_repository(
			github_bot,
			&owner,
			&owner_repo,
			&contributor,
			&contributor_repo,
			&contributor_branch,
		)
		.await?;

		log::info!("Companion updated; waiting for checks on {}", html_url);
		wait_to_merge(
			github_bot,
			db,
			&companion_update_result.head_sha,
			MergeRequest {
				contributor,
				contributor_repo,
				owner,
				owner_repo,
				number,
				html_url,
				requested_by: "parity-processbot[bot]".to_string(),
			},
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
) -> Result<()> {
	if merge_done_in == "substrate" {
		log::info!("Checking for companion.");
		if let Some((html_url, owner, repo, number)) =
			pr.body.as_ref().map(|body| companion_parse(body)).flatten()
		{
			log::info!("Found companion {}", html_url);
			perform_companion_update(github_bot, db, &owner, &repo, number)
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
) -> Result<()> {
	detect_then_update_companion(github_bot, merge_done_in, pr, db)
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
