use regex::Regex;
use snafu::ResultExt;
use tokio::process::Command;

use crate::{error::*, github_bot::GithubBot, Result};

pub async fn companion_update(
	github_bot: &GithubBot,
	base_owner: &str,
	base_repo: &str,
	head_owner: &str,
	head_repo: &str,
	branch: &str,
) -> Result<Option<String>> {
	let res = companion_update_inner(
		github_bot, base_owner, base_repo, head_owner, head_repo, branch,
	)
	.await;
	// checkout origin master
	log::info!("Checking out master.");
	Command::new("git")
		.arg("checkout")
		.arg("master")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// delete temp branch
	log::info!("Deleting temp branch.");
	Command::new("git")
		.arg("branch")
		.arg("-D")
		.arg("temp-branch")
		.current_dir(format!("./{}", base_repo))
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
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	res
}

async fn companion_update_inner(
	github_bot: &GithubBot,
	base_owner: &str,
	base_repo: &str,
	head_owner: &str,
	head_repo: &str,
	branch: &str,
) -> Result<Option<String>> {
	let token = github_bot.client.auth_key().await?;
	let mut updated_sha = None;
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
	// checkout origin master
	log::info!("Checking out master.");
	Command::new("git")
		.arg("checkout")
		.arg("master")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// pull origin master
	log::info!("Pulling master.");
	Command::new("git")
		.arg("pull")
		.arg("-v")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
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
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// fetch temp
	log::info!("Fetching temp.");
	Command::new("git")
		.arg("fetch")
		.arg("-v")
		.arg("temp")
		.current_dir(format!("./{}", base_repo))
		.spawn()
		.context(Tokio)?
		.await
		.context(Tokio)?;
	// checkout temp branch
	log::info!("Checking out head branch.");
	let checkout = Command::new("git")
		.arg("checkout")
		.arg("-b")
		.arg("temp-branch")
		.arg(format!("temp/{}", branch))
		.current_dir(format!("./{}", base_repo))
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
			.arg("--no-edit")
			.current_dir(format!("./{}", base_repo))
			.spawn()
			.context(Tokio)?
			.await
			.context(Tokio)?;
		if merge_master.success() {
			// update
			log::info!("Updating substrate.");
			Command::new("cargo")
				.arg("update")
				.arg("-vp")
				.arg("sp-io")
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
			// commit
			log::info!("Committing changes.");
			Command::new("git")
				.arg("commit")
				.arg("-a")
				.arg("-m")
				.arg("'Update substrate'")
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
			// push
			log::info!("Pushing changes.");
			Command::new("git")
				.arg("push")
				.arg("-v")
				.arg("temp")
				.arg(format!("temp-branch:{}", branch))
				.current_dir(format!("./{}", base_repo))
				.spawn()
				.context(Tokio)?
				.await
				.context(Tokio)?;
			// rev-parse
			log::info!("Parsing SHA.");
			let output = Command::new("git")
				.arg("rev-parse")
				.arg("HEAD")
				.current_dir(format!("./{}", base_repo))
				.output()
				.await
				.context(Tokio)?;
			updated_sha = Some(
				String::from_utf8(output.stdout)
					.context(Utf8)?
					.trim()
					.to_string(),
			);
		}
	}
	Ok(updated_sha)
}

pub fn companion_parse(body: &str) -> Option<(String, String, String, i64)> {
	companion_parse_long(body).or(companion_parse_short(body))
}

fn companion_parse_long(body: &str) -> Option<(String, String, String, i64)> {
	let re = Regex::new(
		r"companion.*(?P<html_url>https://github.com/(?P<owner>[[:alpha:]]+)/(?P<repo>[[:alpha:]]+)/pull/(?P<number>[[:digit:]]+))"
	)
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
	let re = Regex::new(
		r"companion.*: (?P<owner>[[:alpha:]]+)/(?P<repo>[[:alpha:]]+)#(?P<number>[[:digit:]]+)"
	)
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_companion_parse() {
		assert_eq!(
			companion_parse(
				"companion: https://github.com/paritytech/polkadot/pull/1234"
			),
			Some((
				"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
				"paritytech".to_owned(),
				"polkadot".to_owned(),
				1234
			))
		);
		assert_eq!(
			companion_parse(
				"\nthis is a companion pr https://github.com/paritytech/polkadot/pull/1234"
			),
			Some((
				"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
				"paritytech".to_owned(),
				"polkadot".to_owned(),
				1234
			))
		);
		assert_eq!(
			companion_parse(
				"\nthis is some other pr https://github.com/paritytech/polkadot/pull/1234"
			),
            None,
		);
		assert_eq!(
			companion_parse(
				"\nthis is a companion pr https://github.com/paritytech/polkadot/pull/1234/plus+some&other_stuff"
			),
			Some((
				"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
				"paritytech".to_owned(),
				"polkadot".to_owned(),
				1234
			))
		);
		assert_eq!(
			companion_parse("companion\nparitytech/polkadot#1234"),
			None
		);
		assert_eq!(
			companion_parse("companion: paritytech/polkadot#1234"),
			Some((
				"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
				"paritytech".to_owned(),
				"polkadot".to_owned(),
				1234
			))
		);
		assert_eq!(
			companion_parse("companion: paritytech/polkadot/1234"),
			None
		);
		assert_eq!(
			companion_parse("stuff\ncompanion pr: paritytech/polkadot#1234"),
			Some((
				"https://github.com/paritytech/polkadot/pull/1234".to_owned(),
				"paritytech".to_owned(),
				"polkadot".to_owned(),
				1234
			))
		);
	}
}
