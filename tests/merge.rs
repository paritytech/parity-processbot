use insta::assert_snapshot;
use parity_processbot::{
	config::MainConfig,
	github,
	github_bot::GithubBot,
	webhook::{handle_payload, AppState},
	PlaceholderDeserializationItem,
};
use rocksdb::DB;
use std::fs;

mod helpers;

use helpers::{cmd::*, constants::*, read_snapshot, setup::*};

#[tokio::test]
async fn simple_merge_succeeds() {
	let common_setup = common_setup();
	let CommonSetupOutput {
		log_dir,
		github_api_url,
		db_dir,
		owner,
		repo_dir,
		private_key,
		github_app_id,
		repo_name,
		repo_full_name,
		core_devs_team,
		team_leads_team,
		..
	} = &common_setup;

	// Create PR branch
	let pr_branch = "contributor_patches";
	exec(
		"git",
		&["checkout", "-b", pr_branch],
		Some(repo_dir),
		Some(CmdConfiguration::IgnoreStderrStartingWith(&[
			"Switched to a new branch",
		])),
	);

	// Add a commit to the PR's branch
	fs::write(repo_dir.join("foo"), "this file has changed").unwrap();
	exec("git", &["add", "."], Some(repo_dir), None);
	exec(
		"git",
		&["commit", "-m", "change file"],
		Some(repo_dir),
		None,
	);
	let pr_head_sha =
		get_cmd_output("git", &["rev-parse", "HEAD"], Some(&repo_dir));

	// Setup the commit in the API so that the status checks criterion will pass
	setup_commit(&common_setup, &pr_head_sha);

	let repo = github::Repository {
		name: repo_name.to_string(),
		full_name: repo_full_name.clone(),
		owner: owner.clone(),
		html_url: format!(
			"{}/{}",
			URL_PLACEHOLDER_WHICH_DOES_NOT_MATTER, repo_full_name
		),
	};

	let mut next_pr_number: i64 = 0;
	next_pr_number += 1;
	let pr = &setup_pull_request(
		&common_setup,
		&repo,
		&pr_head_sha,
		pr_branch,
		next_pr_number,
	);

	let config = MainConfig {
		installation_login: owner.login.clone(),
		webhook_secret: "does not matter".to_owned(),
		webhook_port: "does not matter".to_string(),
		db_path: db_dir.path().display().to_string(),
		private_key: private_key.clone(),
		webhook_proxy_url: None,
		disable_org_check: false,
		github_api_url: github_api_url.clone(),
		github_app_id: *github_app_id,
		merge_command_delay: 0,
		companion_status_settle_delay: 0,
		core_devs_team: core_devs_team.to_string(),
		team_leads_team: team_leads_team.to_string(),
	};
	let github_bot = GithubBot::new(&config);
	let db = DB::open_default(&config.db_path).unwrap();
	let state = AppState {
		db,
		github_bot,
		config,
	};

	let _ = handle_payload(
		github::Payload::IssueComment {
			action: github::IssueCommentAction::Created,
			comment: github::Comment {
				body: "bot merge".to_string(),
				user: Some(owner.clone()),
			},
			issue: github::WebhookIssueComment {
				number: pr.number,
				html_url: pr.html_url.clone(),
				repository_url: repo.html_url.clone(),
				pull_request: Some(PlaceholderDeserializationItem {}),
			},
		},
		&state,
	)
	.await;

	assert_snapshot!(read_snapshot(
		log_dir.path().to_path_buf(),
		&[&pr_head_sha]
	));
}
