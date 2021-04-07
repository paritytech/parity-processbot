use httptest::{matchers::*, responders::*, Expectation, Server};
use parity_processbot::{
	config::{BotConfig, MainConfig},
	constants::*,
	github, github_bot, gitlab_bot, matrix_bot,
	setup::setup,
};
use serde_json::json;
use std::{
	env, fs,
	path::PathBuf,
	process::{self, Command, Stdio},
};
use tempfile::TempDir;

use super::{cmd::*, *};

pub struct CommonSetupOutput {
	pub log_dir: TempDir,
	pub repositories_dir: TempDir,
	pub git_fetch_url: String,
	pub state: parity_processbot::webhook::AppState,
	pub bot_username: &'static str,
	pub git_daemon_handle: process::Child,
	pub git_daemon_dir: TempDir,
	pub user: github::User,
	pub org: &'static str,
}
pub async fn common_setup() -> CommonSetupOutput {
	let git_daemon_pid_file = env::var("GIT_DAEMON_PID_FILE").unwrap();

	let log_dir = tempfile::tempdir().unwrap();
	flexi_logger::Logger::with_env_or_str("info")
		.log_to_file()
		.directory((&log_dir).path().to_path_buf())
		.duplicate_to_stdout(flexi_logger::Duplicate::All)
		.start()
		.unwrap();

	let repositories_dir = tempfile::tempdir().unwrap();
	parity_processbot::utils::REPOSITORIES_DIR
		.set(repositories_dir.path().to_owned())
		.unwrap();

	let bot_username = "bot";
	let org = "org";
	let db_dir = tempfile::tempdir().unwrap();

	let git_daemon_dir = tempfile::tempdir().unwrap();
	clean_directory(git_daemon_dir.path().to_path_buf());
	let git_daemon_port = get_available_port().unwrap();
	let git_fetch_url = format!("git://127.0.0.1:{}", git_daemon_port);

	let placeholder_private_key = "-----BEGIN PRIVATE KEY-----
	MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDJETqse41HRBsc
	7cfcq3ak4oZWFCoZlcic525A3FfO4qW9BMtRO/iXiyCCHn8JhiL9y8j5JdVP2Q9Z
	IpfElcFd3/guS9w+5RqQGgCR+H56IVUyHZWtTJbKPcwWXQdNUX0rBFcsBzCRESJL
	eelOEdHIjG7LRkx5l/FUvlqsyHDVJEQsHwegZ8b8C0fz0EgT2MMEdn10t6Ur1rXz
	jMB/wvCg8vG8lvciXmedyo9xJ8oMOh0wUEgxziVDMMovmC+aJctcHUAYubwoGN8T
	yzcvnGqL7JSh36Pwy28iPzXZ2RLhAyJFU39vLaHdljwthUaupldlNyCfa6Ofy4qN
	ctlUPlN1AgMBAAECggEAdESTQjQ70O8QIp1ZSkCYXeZjuhj081CK7jhhp/4ChK7J
	GlFQZMwiBze7d6K84TwAtfQGZhQ7km25E1kOm+3hIDCoKdVSKch/oL54f/BK6sKl
	qlIzQEAenho4DuKCm3I4yAw9gEc0DV70DuMTR0LEpYyXcNJY3KNBOTjN5EYQAR9s
	2MeurpgK2MdJlIuZaIbzSGd+diiz2E6vkmcufJLtmYUT/k/ddWvEtz+1DnO6bRHh
	xuuDMeJA/lGB/EYloSLtdyCF6sII6C6slJJtgfb0bPy7l8VtL5iDyz46IKyzdyzW
	tKAn394dm7MYR1RlUBEfqFUyNK7C+pVMVoTwCC2V4QKBgQD64syfiQ2oeUlLYDm4
	CcKSP3RnES02bcTyEDFSuGyyS1jldI4A8GXHJ/lG5EYgiYa1RUivge4lJrlNfjyf
	dV230xgKms7+JiXqag1FI+3mqjAgg4mYiNjaao8N8O3/PD59wMPeWYImsWXNyeHS
	55rUKiHERtCcvdzKl4u35ZtTqQKBgQDNKnX2bVqOJ4WSqCgHRhOm386ugPHfy+8j
	m6cicmUR46ND6ggBB03bCnEG9OtGisxTo/TuYVRu3WP4KjoJs2LD5fwdwJqpgtHl
	yVsk45Y1Hfo+7M6lAuR8rzCi6kHHNb0HyBmZjysHWZsn79ZM+sQnLpgaYgQGRbKV
	DZWlbw7g7QKBgQCl1u+98UGXAP1jFutwbPsx40IVszP4y5ypCe0gqgon3UiY/G+1
	zTLp79GGe/SjI2VpQ7AlW7TI2A0bXXvDSDi3/5Dfya9ULnFXv9yfvH1QwWToySpW
	Kvd1gYSoiX84/WCtjZOr0e0HmLIb0vw0hqZA4szJSqoxQgvF22EfIWaIaQKBgQCf
	34+OmMYw8fEvSCPxDxVvOwW2i7pvV14hFEDYIeZKW2W1HWBhVMzBfFB5SE8yaCQy
	pRfOzj9aKOCm2FjjiErVNpkQoi6jGtLvScnhZAt/lr2TXTrl8OwVkPrIaN0bG/AS
	aUYxmBPCpXu3UjhfQiWqFq/mFyzlqlgvuCc9g95HPQKBgAscKP8mLxdKwOgX8yFW
	GcZ0izY/30012ajdHY+/QK5lsMoxTnn0skdS+spLxaS5ZEO4qvPVb8RAoCkWMMal
	2pOhmquJQVDPDLuZHdrIiKiDM20dy9sMfHygWcZjQ4WSxf/J7T9canLZIXFhHAZT
	3wc9h4G8BBCtWN2TN/LsGZdB
	-----END PRIVATE KEY-----"
		.as_bytes()
		.to_vec();

	let state = setup(
		Some(MainConfig {
			environment: "".to_string(),
			test_repo: "".to_string(),
			installation_login: bot_username.to_string(),
			webhook_secret: "".to_string(),
			webhook_port: "".to_string(),
			db_path: (&db_dir).path().display().to_string(),
			bamboo_token: "".to_string(),
			private_key: placeholder_private_key.clone(),
			matrix_homeserver: "".to_string(),
			matrix_access_token: "".to_string(),
			matrix_default_channel_id: "".to_string(),
			main_tick_secs: 0,
			bamboo_tick_secs: 0,
			matrix_silent: true,
			gitlab_hostname: "".to_string(),
			gitlab_project: "".to_string(),
			gitlab_job_name: "".to_string(),
			gitlab_private_token: "".to_string(),
			github_app_id: 1,
		}),
		Some(BotConfig {
			status_failure_ping: 0,
			issue_not_addressed_ping: 0,
			issue_not_assigned_to_pr_author_ping: 0,
			no_project_author_is_core_ping: 0,
			no_project_author_is_core_close_pr: 0,
			no_project_author_unknown_close_pr: 0,
			project_confirmation_timeout: 0,
			review_request_ping: 0,
			private_review_reminder_ping: 0,
			public_review_reminder_ping: 0,
			public_review_reminder_delay: 0,
			min_reviewers: 0,
			core_sorting_repo_name: "".to_string(),
			logs_room_id: "".to_string(),
		}),
		Some(matrix_bot::MatrixBot::new_placeholder_for_testing()),
		Some(gitlab_bot::GitlabBot::new_placeholder_for_testing()),
		Some(github_bot::GithubBot::new_for_testing(
			placeholder_private_key.clone(),
			bot_username,
			&git_fetch_url,
		)),
		false,
	)
	.await
	.unwrap();

	let git_daemon_port = get_available_port().unwrap();
	let git_daemon_handle = Command::new("git")
		.arg("daemon")
		.arg(format!("--port={}", git_daemon_port))
		.arg("--base-path=.")
		.arg("--export-all")
		.arg("--enable=receive-pack")
		.stdout(Stdio::null())
		.current_dir((&git_daemon_dir).path())
		.spawn()
		.unwrap();
	fs::write(git_daemon_pid_file, format!("{}", git_daemon_handle.id()))
		.unwrap();

	let user = github::User {
		login: "foo".to_string(),
		type_field: Some(github::UserType::User),
	};

	CommonSetupOutput {
		log_dir,
		repositories_dir,
		git_fetch_url,
		state,
		bot_username,
		git_daemon_handle,
		org,
		user,
		git_daemon_dir,
	}
}

pub struct CreateOwnerOutput {
	pub branch: &'static str,
	pub repo_name: &'static str,
	pub repo_dir: PathBuf,
	pub user: github::User,
	pub repo: github::Repository,
	pub head_sha: String,
}
pub fn create_owner(out: &CommonSetupOutput) -> CreateOwnerOutput {
	let repo_name = "project";
	let branch = "master";
	let repo_dir = out.git_daemon_dir.path().join(out.org).join(repo_name);
	let user = github::User {
		login: out.org.to_string(),
		type_field: Some(github::UserType::User),
	};
	let repo = github::Repository {
		name: repo_name.to_string(),
		full_name: Some(format!("{}/{}", out.org, repo_name)),
		owner: Some(user.clone()),
		html_url: "".to_string(),
	};
	fs::create_dir_all(&repo_dir).unwrap();
	run_cmd("git", &["init", "-b", branch], &repo_dir, None);
	fs::write(
		&repo_dir.join("Cargo.toml"),
		r#"
	[package]
	name = "project"
	version = "0.0.1"
	authors = ["owner <owner@owner.com>"]
	description = "project"
	"#,
	)
	.unwrap();

	let src_dir = &repo_dir.join("src");
	fs::create_dir_all(&src_dir).unwrap();
	fs::write((&src_dir).join("main.rs"), "fn main() {}").unwrap();
	run_cmd("git", &["add", "."], &repo_dir, None);
	run_cmd("git", &["commit", "-m", "init"], &repo_dir, None);
	let head_sha = get_cmd_output("git", &["rev-parse", "HEAD"], &repo_dir);

	CreateOwnerOutput {
		branch,
		repo_name,
		repo_dir,
		user,
		repo,
		head_sha,
	}
}

pub struct SetupGithubAPIOutput {
	pub github_api: httptest::Server,
	pub api_root: String,
}
pub fn setup_github_api(
	bot_username: &str,
	org: &str,
	org_members: Vec<github::User>,
) -> SetupGithubAPIOutput {
	let github_api = Server::run();
	let api_root = github_api.url("").to_string();
	// Trims off the slash at the end
	let api_root = api_root[0..api_root.len() - 1].to_string();
	parity_processbot::github::BASE_API_URL
		.set((&api_root).to_owned())
		.unwrap();
	let installation_id = 1;
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			"/app/installations",
		))
		.respond_with(json_encoded(vec![github::Installation {
			id: installation_id,
			account: github::User {
				login: bot_username.to_string(),
				type_field: Some(github::UserType::Bot),
			},
		}])),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"POST",
			format!("/app/installations/{}/access_tokens", installation_id),
		))
		.respond_with(json_encoded(github::InstallationToken {
			token: "DOES_NOT_MATTER".to_string(),
			expires_at: None,
		})),
	);
	for user in org_members.iter() {
		github_api.expect(
			Expectation::matching(request::method_path(
				"GET",
				format!("/orgs/{}/members/{}", org, user.login),
			))
			.times(0..)
			.respond_with(
				status_code(204)
					.append_header("Content-Type", "application/json")
					.body(serde_json::to_string(&json!({})).unwrap()),
			),
		);
	}

	SetupGithubAPIOutput {
		github_api,
		api_root,
	}
}

pub struct SetupPullRequestOutput {
	pub api_path: String,
	pub api_url: String,
	pub html_url: String,
	pub issue_api_path: String,
	pub repository_url: String,
	pub url: String,
	pub number: i64,
}
pub fn setup_pull_request(
	github_api: &Server,
	api_root: &str,
	org: &str,
	owner: &CreateOwnerOutput,
	head_sha: &str,
	branch: &str,
	expected_times: usize,
) -> SetupPullRequestOutput {
	let number: i64 = 1;
	let repository_url =
		format!("https://github.com/{}/{}", org, owner.repo_name);
	let html_url = format!("{}/pull/{}", &repository_url, number);
	let url = format!("{}/pull/{}", &repository_url, number);
	let api_path =
		format!("/repos/{}/{}/pull/{}", org, owner.repo_name, number);
	let issue_api_path =
		format!("/repos/{}/{}/issues/{}", org, owner.repo_name, number);
	let api_url = format!(
		"{}/repos/{}/{}/pulls/{}",
		api_root, org, owner.repo_name, number
	);

	let dir_path_1: &'static PathBuf =
		&*Box::leak(Box::new(owner.repo_dir.clone()));
	let branch_1: &'static String = &*Box::leak(Box::new(branch.to_string()));
	let owner_branch_1: &'static String =
		&*Box::leak(Box::new(owner.branch.to_string()));
	let owner_repo_name_1: &'static String =
		&*Box::leak(Box::new(owner.repo_name.to_string()));
	let org_1: &'static String = &*Box::leak(Box::new(org.to_string()));
	github_api.expect(
		Expectation::matching(request::method_path(
			"PUT",
			format!(
				"/repos/{}/{}/pulls/{}/merge",
				org_1, owner_repo_name_1, number
			),
		))
		.times(expected_times)
		.respond_with(move || {
			run_cmd(
				"git",
				&["checkout", branch_1],
				dir_path_1,
				Some(CmdConfiguration::SilentStderrStartingWith(&[
					"Switched to branch",
				])),
			);
			let tmp_branch_name = "tmp";
			run_cmd(
				"git",
				&["checkout", "-b", tmp_branch_name],
				dir_path_1,
				Some(CmdConfiguration::SilentStderrStartingWith(&[
					"Switched to a new branch",
				])),
			);
			let merge_output =
				get_cmd_output("git", &["merge", owner_branch_1], dir_path_1);
			// Merge is only successful if contributor branch is up-to-date with master; simulates
			// the failure caused by Github API when the PR is outdated due to branch protection
			// rules.
			let result = if merge_output == "Already up to date." {
				status_code(200)
					.append_header("Content-Type", "application/json")
					.body(serde_json::to_string(&json!({})).unwrap())
			} else {
				status_code(405)
					.append_header("Content-Type", "application/json")
					.body(
						serde_json::to_string(
							&json!({ "message": "Pull Request is not mergeable" }),
						)
						.unwrap(),
					)
			};
			run_cmd(
				"git",
				&["merge", "--abort"],
				dir_path_1,
				Some(CmdConfiguration::SilentStderrStartingWith(&[
					"fatal: There is no merge to abort",
				])),
			);
			run_cmd(
				"git",
				&["checkout", owner_branch_1],
				dir_path_1,
				Some(CmdConfiguration::SilentStderrStartingWith(&[
					"Switched to branch",
				])),
			);
			run_cmd(
				"git",
				&["branch", "-D", tmp_branch_name],
				dir_path_1,
				None,
			);
			result
		}),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/repos/{}/{}/pulls/{}", org, owner.repo_name, number),
		))
		.respond_with(json_encoded(github::PullRequest {
			body: None,
			number,
			labels: vec![],
			mergeable: Some(true),
			html_url: html_url.clone(),
			url: api_url.clone(),
			user: Some(owner.user.clone()),
			repository: Some(owner.repo.clone()),
			base: github::Base {
				ref_field: Some(owner.branch.to_string()),
				sha: Some(head_sha.to_string()),
				repo: Some(github::HeadRepo {
					name: owner.repo_name.to_string(),
					owner: Some(owner.user.clone()),
				}),
			},
			head: Some(github::Head {
				ref_field: Some(branch.to_string()),
				sha: Some(head_sha.to_string()),
				repo: Some(github::HeadRepo {
					name: owner.repo_name.to_string(),
					owner: Some(owner.user.clone()),
				}),
			}),
		})),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("{}/reviews", api_path,),
		))
		.respond_with(json_encoded(vec![github::Review {
			id: 1,
			user: Some(owner.user.clone()),
			state: Some(github::ReviewState::Approved),
		}])),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"POST",
			format!("{}/comments", issue_api_path,),
		))
		.respond_with(
			status_code(201)
				.append_header("Content-Type", "application/json")
				.body(serde_json::to_string(&json!({})).unwrap()),
		),
	);

	SetupPullRequestOutput {
		api_path,
		api_url,
		html_url,
		issue_api_path,
		url,
		number,
		repository_url,
	}
}

pub fn setup_commit(
	github_api: &Server,
	org: &str,
	owner: &CreateOwnerOutput,
	sha: &str,
) {
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!(
				"/repos/{}/{}/commits/{}/status",
				org, owner.repo_name, sha
			),
		))
		.respond_with(json_encoded(github::CombinedStatus {
			state: github::StatusState::Success,
			statuses: vec![github::Status {
				id: 1,
				context: "DOES_NOT_MATTER".to_string(),
				state: github::StatusState::Success,
			}],
		})),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!(
				"/repos/{}/{}/commits/{}/check-runs",
				org, owner.repo_name, sha
			),
		))
		.respond_with(json_encoded(github::CheckRuns {
			check_runs: vec![github::CheckRun {
				id: 1,
				name: "DOES_NOT_MATTER".to_string(),
				status: github::CheckRunStatus::Completed,
				conclusion: Some(github::CheckRunConclusion::Success),
				head_sha: sha.to_string(),
			}],
		})),
	);
}

pub fn setup_coredevs(
	github_api: &Server,
	org: &str,
	users: Vec<github::User>,
) {
	let team_id = 1;
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/orgs/{}/teams/{}", org, CORE_DEVS_GROUP),
		))
		.times(0..)
		.respond_with(json_encoded(github::Team { id: team_id })),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/teams/{}/members", team_id),
		))
		.times(0..)
		.respond_with(json_encoded(users)),
	);
}

pub fn setup_substrateteamleads(
	github_api: &Server,
	org: &str,
	users: Vec<github::User>,
) {
	let team_id = 2;
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/orgs/{}/teams/{}", org, SUBSTRATE_TEAM_LEADS_GROUP),
		))
		.times(0..)
		.respond_with(json_encoded(github::Team { id: team_id })),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/teams/{}/members", team_id),
		))
		.times(0..)
		.respond_with(json_encoded(users)),
	);
}
