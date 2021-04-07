use insta::assert_snapshot;
use parity_processbot::{github, webhook::handle_payload};
use std::fs;

mod utils;

use utils::{cmd::*, common::*, *};

#[tokio::test]
async fn recovers_from_outdated_master() {
	let common_setup_result = &common_setup().await;
	let CommonSetupOutput {
		log_dir,
		git_fetch_url,
		state,
		org,
		user,
		bot_username,
		..
	} = common_setup_result;

	let owner = &create_owner(&common_setup_result);
	let pr_branch = "contributor_patches";

	// Create PR branch
	run_cmd(
		"git",
		&["checkout", "-b", pr_branch],
		&owner.repo_dir,
		Some(CmdConfiguration::SilentStderrStartingWith(&[
			"Switched to a new branch",
		])),
	);
	let pr_head_sha =
		get_cmd_output("git", &["rev-parse", "HEAD"], &owner.repo_dir);

	// Commit on master to make the PR branch outdated
	run_cmd(
		"git",
		&["checkout", owner.branch],
		&owner.repo_dir,
		Some(CmdConfiguration::SilentStderrStartingWith(&[
			"Switched to branch",
		])),
	);
	fs::write(
		owner.repo_dir.join("src").join("main.rs"),
		"fn main() {println!(\"CHANGED!\");}",
	)
	.unwrap();
	run_cmd("git", &["add", "."], &owner.repo_dir, None);
	run_cmd(
		"git",
		&["commit", "-m", "make contributor branch outdated"],
		&owner.repo_dir,
		None,
	);

	let SetupGithubAPIOutput {
		github_api,
		api_root,
	} = &setup_github_api(bot_username, org, vec![user.clone()]);
	setup_coredevs(github_api, org, vec![user.clone()]);
	setup_substrateteamleads(github_api, org, vec![user.clone()]);
	setup_commit(github_api, org, &owner, &pr_head_sha);
	let pr = &setup_pull_request(
		github_api,
		api_root,
		org,
		&owner,
		&pr_head_sha,
		pr_branch,
		2,
	);

	handle_payload(
		github::Payload::IssueComment {
			action: github::IssueCommentAction::Created,
			comment: github::Comment {
				body: "bot merge".to_string(),
				user: Some(owner.user.clone()),
			},
			issue: github::Issue {
				number: pr.number,
				body: None,
				html_url: pr.html_url.clone(),
				repository_url: Some(pr.repository_url.to_string()),
				pull_request: Some(github::IssuePullRequest {}),
				repository: Some(owner.repo.clone()),
				user: Some(owner.user.clone()),
			},
		},
		state,
	)
	.await
	.unwrap();

	assert_snapshot!(read_snapshot(
		(&log_dir).path().to_path_buf(),
		&[git_fetch_url]
	));
}
