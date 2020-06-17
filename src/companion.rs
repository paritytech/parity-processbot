use futures_util::future::FutureExt;
use tokio::process::Command;

use crate::github_bot::GithubBot;

pub async fn companion_update(
	github_bot: &GithubBot,
	owner: &str,
	repo: &str,
	branch: &str,
) -> anyhow::Result<()> {
	let token = github_bot.client.auth_key().await?;
	let child = Command::new("rustup")
		.arg("update")
		.spawn()
		.expect("spawn rustup")
		.then(|_| {
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
				.expect("spawn clone")
				.then(|_| {
					Command::new("cargo")
						.arg("update")
						.arg("-vp")
						.arg("sp-io")
						.current_dir("./repo")
						.spawn()
						.expect("spawn update")
						.then(|_| {
							Command::new("git")
								.arg("commit")
								.arg("-a")
								.arg("-m")
								.arg("'Update substrate'")
								.current_dir("./repo")
								.spawn()
								.expect("spawn commit")
								.then(|_| {
									Command::new("git")
										.arg("push")
										.arg("-vn")
										.current_dir("./repo")
										.spawn()
										.expect("spawn push")
										.then(|_| {
											Command::new("rm")
												.arg("-rf")
												.arg("repo")
												.spawn()
												.expect("spawn repo")
										})
								})
						})
				})
		});

	// Await until the future (and the command) completes
	let status = child.await?;
	println!("the command exited with: {}", status);

	return Ok(());
}
