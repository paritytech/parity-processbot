use regex::Regex;
use tokio::process::Command;

use crate::github_bot::GithubBot;

pub async fn companion_update(
	github_bot: &GithubBot,
	owner: &str,
	repo: &str,
	branch: &str,
	home: &str,
) -> anyhow::Result<()> {
	let token = github_bot.client.auth_key().await?;
	Command::new("rustup")
		.arg("update")
		.current_dir(&format!("{}/.cargo/bin", home))
		.spawn()?
		.await?;
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
		.spawn()?
		.await?;
	Command::new("cargo")
		.arg("update")
		.arg("-vp")
		.arg("sp-io")
		.current_dir("./repo")
		.spawn()?
		.await?;
	Command::new("git")
		.arg("commit")
		.arg("-a")
		.arg("-m")
		.arg("'Update substrate'")
		.current_dir("./repo")
		.spawn()?
		.await?;
	Command::new("git")
		.arg("push")
		.arg("-vn")
		.current_dir("./repo")
		.spawn()?
		.await?;
	Command::new("rm").arg("-rf").arg("repo").spawn()?.await?;
	Ok(())
}

async fn companion_number(body: &str) -> Option<i64> {
	let re = Regex::new(
		r"^https://github.com/paritytech/polkadot/pull/([[:digit:]]+)"
	)
	.unwrap();
    dbg!(re.find(body).unwrap());
//	while re.captures_iter(&s).count() > 0 {
//		s = dbg!(re.replace_all(&s, "[$1-$2").to_string());
//	}
    Ok(false)
}
