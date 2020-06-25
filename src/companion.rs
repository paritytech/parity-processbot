use anyhow::Context;
use regex::Regex;
use tokio::process::Command;

use crate::github_bot::GithubBot;

pub async fn companion_update(
	github_bot: &GithubBot,
	owner: &str,
	repo: &str,
	branch: &str,
) -> anyhow::Result<()> {
	let token = github_bot.client.auth_key().await?;
	Command::new("rustup")
		.arg("update")
		.spawn()
		.context("spawn rustup update")?
		.await
		.context("rustup update")?;
	Command::new("git")
		.arg("clone")
		.arg("-vb")
		.arg(branch)
		.arg(format!(
			"https://x-access-token:{token}@github.com/{owner}/{repo}.git",
			token = token,
			owner = owner,
			repo = repo,
		))
		.arg("repo")
		.spawn()
		.context("spawn git clone")?
		.await
		.context("git clone")?;
	Command::new("cargo")
		.arg("update")
		.arg("-vp")
		.arg("sp-io")
		.current_dir("./repo")
		.spawn()
		.context("spawn cargo update")?
		.await
		.context("cargo update")?;
	Command::new("git")
		.arg("commit")
		.arg("-a")
		.arg("-m")
		.arg("'Update substrate'")
		.current_dir("./repo")
		.spawn()
		.context("spawn git commit")?
		.await
		.context("git commit")?;
	Command::new("git")
		.arg("push")
		.arg("-v")
		.current_dir("./repo")
		.spawn()
		.context("spawn git push")?
		.await
		.context("git push")?;
	Command::new("rm")
		.arg("-rf")
		.arg("repo")
		.spawn()
		.context("spawn rm")?
		.await
		.context("rm")?;
	Ok(())
}

pub fn companion_parse(body: &str) -> Option<(String, String, String, i64)> {
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
	}
}
