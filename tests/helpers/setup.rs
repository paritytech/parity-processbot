use httptest::{matchers::*, responders::*, Expectation, Server};
use parity_processbot::{self, github};
use serde_json::json;
use std::{
	env, fs,
	io::Write,
	path::PathBuf,
	process::{self, Command, Stdio},
};
use tempfile::TempDir;

use super::{cmd::*, constants::*, *};

pub struct CommonSetupOutput {
	pub log_dir: TempDir,
	pub db_dir: TempDir,
	pub git_daemon_handle: process::Child,
	pub git_daemon_dir: TempDir,
	pub private_key: Vec<u8>,
	pub github_api: Server,
	pub github_api_url: String,
	pub owner: github::User,
	pub repo_name: &'static str,
	pub repo_dir: PathBuf,
	pub repo_full_name: String,
	pub github_app_id: usize,
	pub next_team_id: i64,
}
pub fn common_setup() -> CommonSetupOutput {
	let git_daemon_base_path_file =
		env::var("GIT_DAEMON_BASE_PATH_FILE").unwrap();

	let log_dir = tempfile::tempdir().unwrap();
	flexi_logger::Logger::with_env_or_str("info")
		.log_to_file()
		.directory((&log_dir).path().to_path_buf())
		.duplicate_to_stdout(flexi_logger::Duplicate::All)
		.start()
		.unwrap();

	let db_dir = tempfile::tempdir().unwrap();

	let git_daemon_dir = tempfile::tempdir().unwrap();
	let git_daemon_dir_path_str = git_daemon_dir.path().display().to_string();
	{
		let mut file = std::fs::OpenOptions::new()
			.write(true)
			.append(true)
			.open(git_daemon_base_path_file)
			.unwrap();
		writeln!(file, "{}", &git_daemon_dir_path_str).unwrap();
	}
	clean_directory(git_daemon_dir.path().to_path_buf());

	let owner = github::User {
		login: "owner".to_string(),
		type_field: Some(github::UserType::User),
	};
	let private_key = "
-----BEGIN PRIVATE KEY-----
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
-----END PRIVATE KEY-----
  "
	.as_bytes()
	.to_vec();

	// Set up the Git repository with an initial commit
	let repo = "repo";
	let repo_full_name = format!("{}/{}", &owner.login, repo);
	let repo_dir = git_daemon_dir.path().join(&owner.login).join(repo);
	let initial_branch = "master";
	fs::create_dir_all(&repo_dir).unwrap();
	initialize_repository(&repo_dir, initial_branch);

	let github_api = Server::run();
	let github_api_url = {
		let url = github_api.url("").to_string();
		url[0..url.len() - 1].to_string()
	};

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			"/app/installations",
		))
		.times(0..)
		.respond_with(json_encoded(vec![github::Installation {
			id: I64_PLACEHOLDER_WHICH_DOES_NOT_MATTER,
			account: github::User {
				login: owner.login.clone(),
				type_field: Some(github::UserType::Bot),
			},
		}])),
	);
	github_api.expect(
		Expectation::matching(request::method_path(
			"POST",
			format!(
				"/app/installations/{}/access_tokens",
				I64_PLACEHOLDER_WHICH_DOES_NOT_MATTER
			),
		))
		.times(0..)
		.respond_with(json_encoded(github::InstallationToken {
			token: "does not matter".to_string(),
			expires_at: None,
		})),
	);

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!(
				"/repos/{}/{}/contents/{}",
				&owner.login,
				repo,
				parity_processbot::constants::PROCESS_FILE
			),
		))
		.times(0..)
		.respond_with(|| status_code(200).body(base64::encode("[]"))),
	);

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/orgs/{}/members/{}", &owner.login, &owner.login),
		))
		.times(0..)
		.respond_with(
			status_code(204)
				.append_header("Content-Type", "application/json")
				.body(serde_json::to_string(&json!({})).unwrap()),
		),
	);

	let mut next_team_id = 0;
	for team in &[
		parity_processbot::constants::CORE_DEVS_GROUP,
		parity_processbot::constants::SUBSTRATE_TEAM_LEADS_GROUP,
	] {
		next_team_id += 1;
		setup_team(
			&github_api,
			&owner.login,
			team,
			next_team_id,
			vec![owner.clone()],
		);
	}

	let git_daemon_port = get_available_port().unwrap();
	let git_daemon_handle = Command::new("git")
		.arg("daemon")
		.arg(format!("--port={}", git_daemon_port))
		.arg(format!("--base-path={}", git_daemon_dir_path_str))
		.arg("--export-all")
		.arg("--enable=receive-pack")
		.stdout(Stdio::null())
		.current_dir((&git_daemon_dir).path())
		.spawn()
		.unwrap();

	CommonSetupOutput {
		log_dir,
		git_daemon_handle,
		git_daemon_dir,
		github_api,
		github_api_url,
		db_dir,
		repo_dir,
		github_app_id: USIZE_PLACEHOLDER_WHICH_DOES_NOT_MATTER,
		owner,
		repo_name: repo,
		repo_full_name,
		private_key,
		next_team_id,
	}
}

pub fn setup_team(
	github_api: &Server,
	org: &str,
	team: &str,
	team_id: i64,
	users: Vec<github::User>,
) {
	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("/orgs/{}/teams/{}", org, team),
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

pub fn setup_commit(setup: &CommonSetupOutput, sha: &str) {
	let CommonSetupOutput {
		owner,
		repo_name,
		github_api,
		..
	} = setup;

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!(
				"/repos/{}/{}/commits/{}/status",
				&owner.login, repo_name, sha
			),
		))
		.times(0..)
		.respond_with(json_encoded(github::CombinedStatus {
			statuses: vec![github::Status {
				id: 1,
				context: "does not matter".to_string(),
				description: Some("does not matter".to_string()),
				state: github::StatusState::Success,
			}],
		})),
	);

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!(
				"/repos/{}/{}/commits/{}/check-runs",
				&owner.login, repo_name, sha
			),
		))
		.times(0..)
		.respond_with(json_encoded(github::CheckRuns {
			check_runs: vec![github::CheckRun {
				id: 1,
				name: "does not matter".to_string(),
				status: github::CheckRunStatus::Completed,
				conclusion: Some(github::CheckRunConclusion::Success),
				head_sha: sha.to_string(),
			}],
		})),
	);
}

pub struct SetupPullRequestOutput {
	pub url: String,
	pub html_url: String,
	pub number: i64,
}
pub fn setup_pull_request(
	setup: &CommonSetupOutput,
	repo: &github::Repository,
	head_sha: &str,
	branch: &str,
	number: i64,
) -> SetupPullRequestOutput {
	let CommonSetupOutput {
		github_api,
		github_api_url,
		owner,
		repo_dir,
		..
	} = setup;

	let pr_api_path = &format!("/repos/{}/pulls/{}", &repo.full_name, number);
	let issue_api_path =
		&format!("/repos/{}/issues/{}", &repo.full_name, number);
	let url = format!(
		"{}/repos/{}/pulls/{}",
		github_api_url, &repo.full_name, number
	);
	let html_url = format!("{}/pull/{}", &repo.html_url, number);

	{
		let repo_dir: &'static PathBuf =
			&*Box::leak(Box::new(repo_dir.clone()));
		let branch: &'static String = &*Box::leak(Box::new(branch.to_string()));
		let owner_branch: &'static String =
			&*Box::leak(Box::new(branch.to_string()));
		github_api.expect(
			Expectation::matching(request::method_path(
				"PUT",
				format!("{}/merge", pr_api_path),
			))
			.times(0..)
			.respond_with(move || {
				exec(
					"git",
					&["checkout", branch],
					Some(repo_dir),
					Some(CmdConfiguration::SilentStderrStartingWith(&[
						"Switched to branch",
					])),
				);
				let tmp_branch_name = "tmp";
				exec(
					"git",
					&["checkout", "-b", tmp_branch_name],
					Some(repo_dir),
					Some(CmdConfiguration::SilentStderrStartingWith(&[
						"Switched to a new branch",
					])),
				);
				let merge_output = get_cmd_output(
					"git",
					&["merge", owner_branch],
					Some(repo_dir),
				);
				// Merge is only successful if contributor branch is up-to-date with master; otherwise,
				// simulates the "Pull Request is not mergeable" response (code 405).
				// https://docs.github.com/en/rest/reference/pulls#merge-a-pull-request
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
				exec(
					"git",
					&["merge", "--abort"],
					Some(repo_dir),
					Some(CmdConfiguration::SilentStderrStartingWith(&[
						"fatal: There is no merge to abort",
					])),
				);
				exec(
					"git",
					&["checkout", owner_branch],
					Some(repo_dir),
					Some(CmdConfiguration::SilentStderrStartingWith(&[
						"Switched to branch",
					])),
				);
				exec(
					"git",
					&["branch", "-D", tmp_branch_name],
					Some(repo_dir),
					None,
				);
				result
			}),
		);
	}

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			pr_api_path.to_string(),
		))
		.times(0..)
		.respond_with(json_encoded(github::PullRequest {
			body: None,
			number,
			mergeable: Some(true),
			html_url: html_url.clone(),
			url: url.clone(),
			user: Some(owner.clone()),
			base: github::Base {
				ref_field: branch.to_string(),
				repo: github::BaseRepo {
					name: repo.name.clone(),
					owner: owner.clone(),
				},
			},
			head: github::Head {
				ref_field: branch.to_string(),
				sha: head_sha.to_string(),
				repo: github::HeadRepo {
					name: repo.name.clone(),
					owner: owner.clone(),
				},
			},
			merged: false,
			maintainer_can_modify: true,
		})),
	);

	github_api.expect(
		Expectation::matching(request::method_path(
			"GET",
			format!("{}/reviews", pr_api_path),
		))
		.times(0..)
		.respond_with(json_encoded(vec![github::Review {
			id: 1,
			user: Some(owner.clone()),
			state: Some(github::ReviewState::Approved),
		}])),
	);

	github_api.expect(
		Expectation::matching(request::method_path(
			"POST",
			format!("{}/comments", issue_api_path,),
		))
		.times(0..)
		.respond_with(
			status_code(201)
				.append_header("Content-Type", "application/json")
				.body(serde_json::to_string(&json!({})).unwrap()),
		),
	);

	SetupPullRequestOutput {
		url,
		html_url,
		number,
	}
}
