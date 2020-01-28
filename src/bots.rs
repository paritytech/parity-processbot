use parking_lot::RwLock;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
	constants::*, error, github, github_bot::GithubBot, matrix_bot::MatrixBot,
	process, Result,
};

#[derive(Debug, Clone)]
pub struct BotConfig {
	/// seconds between pings
	pub status_failure_ping: u64,
	/// seconds between pings
	pub issue_not_assigned_to_pr_author_ping: u64,
	/// seconds between pings
	pub no_project_author_is_core_ping: u64,
	/// seconds between pings
	pub no_project_author_not_core_ping: u64,
	/// seconds between pings
	pub unconfirmed_project_ping: u64,
	/// seconds between pings
	pub review_request_ping: u64,
	/// seconds between pings
	pub private_review_reminder_ping: u64,
	/// seconds between pings
	pub public_review_reminder_ping: u64,

	/// seconds before action
	pub no_project_close_pr: u64,
	/// mininum number of reviewers
	pub min_reviewers: usize,
	/// matrix room id to be used when missing project details
	pub fallback_room_id: String, // TODO remove in favour of Config's default_matrix_room_id
	/// name of repo for issues without a project
	pub core_sorting_repo_name: String,
	/// name of project column to which new issues should be attached
	pub project_backlog_column_name: String,
}

impl BotConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();
		Self {
			status_failure_ping: dotenv::var("status_failure_ping")
				.expect("status_failure_ping")
				.parse::<u64>()
				.expect("failed parsing status_failure_ping"),
			issue_not_assigned_to_pr_author_ping: dotenv::var(
				"issue_not_assigned_to_pr_author_ping",
			)
			.expect("issue_not_assigned_to_pr_author_ping")
			.parse::<u64>()
			.expect("failed parsing issue_not_assigned_to_pr_author_ping"),
			no_project_author_is_core_ping: dotenv::var(
				"no_project_author_is_core_ping",
			)
			.expect("no_project_author_is_core_ping")
			.parse::<u64>()
			.expect("failed parsing no_project_author_is_core_ping"),
			no_project_author_not_core_ping: dotenv::var(
				"no_project_author_not_core_ping",
			)
			.expect("no_project_author_not_core_ping")
			.parse::<u64>()
			.expect("failed parsing no_project_author_not_core_ping"),
			unconfirmed_project_ping: dotenv::var("unconfirmed_project_ping")
				.expect("unconfirmed_project_ping")
				.parse::<u64>()
				.expect("failed parsing unconfirmed_project_ping"),
			review_request_ping: dotenv::var("review_request_ping")
				.expect("review_request_ping")
				.parse::<u64>()
				.expect("failed parsing review_request_ping"),
			private_review_reminder_ping: dotenv::var(
				"private_review_reminder_ping",
			)
			.expect("private_review_reminder_ping")
			.parse::<u64>()
			.expect("failed parsing private_review_reminder_ping"),
			public_review_reminder_ping: dotenv::var(
				"public_review_reminder_ping",
			)
			.expect("public_review_reminder_ping")
			.parse::<u64>()
			.expect("failed parsing public_review_reminder_ping"),
			no_project_close_pr: dotenv::var("no_project_close_pr")
				.expect("no_project_close_pr")
				.parse::<u64>()
				.expect("failed parsing no_project_close_pr"),
			min_reviewers: dotenv::var("min_reviewers")
				.expect("min_reviewers")
				.parse::<usize>()
				.expect("failed parsing min_reviewers"),
			fallback_room_id: dotenv::var("fallback_room_id")
				.expect("fallback_room_id"),
			core_sorting_repo_name: dotenv::var("core_sorting_repo_name")
				.expect("core_sorting_repo_name"),
			project_backlog_column_name: dotenv::var(
				"project_backlog_column_name",
			)
			.expect("project_backlog_column_name"),
		}
	}
}

pub struct Bot {
	pub db: Arc<RwLock<DB>>,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub core_devs: Vec<github::User>,
	pub github_to_matrix: HashMap<String, String>,
	pub config: BotConfig,
}

impl Bot {
	pub fn new(
		db: Arc<RwLock<DB>>,
		github_bot: GithubBot,
		matrix_bot: MatrixBot,
		core_devs: Vec<github::User>,
		github_to_matrix: HashMap<String, String>,
	) -> Bot {
		Bot {
			db,
			github_bot,
			matrix_bot,
			core_devs,
			github_to_matrix,
			config: BotConfig::from_env(),
		}
	}

	pub async fn update(&self) -> Result<()> {
		for repo in self.github_bot.repositories().await?.iter() {
			if let Ok(repo_projects) =
				self.github_bot.projects(&repo.name).await
			{
				if let Ok(contents) =
					self.github_bot.contents(&repo.name, "Process.toml").await
				{
					if let Ok(process) =
						process::process_from_contents(contents)
					{
						// projects in Process.toml are useless if they do not match a project
						// in the repo
						let projects_with_process = process
							.into_iter()
							.map(
								|(key, process_info): (
									String,
									process::ProcessInfo,
								)| {
									(
										repo_projects
											.iter()
											.find(|rp| rp.name == key)
											.cloned(),
										process_info,
									)
								},
							)
							.collect::<Vec<(
								Option<github::Project>,
								process::ProcessInfo,
							)>>();

						if projects_with_process.len() > 0 {
							for issue in self
								.github_bot
								.repository_issues(&repo)
								.await?
								.iter()
								.skip(1)
							{
								// if issue.pull_request.is_some() then this issue is a pull
								// request, which we treat differently
								if issue.pull_request.is_none() {
									match self
										.handle_issue(
											projects_with_process.as_ref(),
											&repo,
											&issue,
										)
										.await
									{
										Err(e) => {
											log::error!(
                                            "Error handling issue #{issue_number} in repo {repo_name}: {error}",
                                            issue_number = issue.number,
                                            repo_name = repo.name,
                                            error = e
                                        );
										}
										_ => {}
									}
								}
							}

							for pr in
								self.github_bot.pull_requests(&repo).await?
							{
								match self
									.handle_pull_request(
										projects_with_process.as_ref(),
										&repo,
										&pr,
									)
									.await
								{
									Err(e) => {
										log::error!(
                                        "Error handling pull request #{issue_number} in repo {repo_name}: {error}",
                                        issue_number = pr.number.unwrap(),
                                        repo_name = repo.name,
                                        error = e
                                    );
									}
									_ => {}
								}
							}
						} else {
							// Process.toml does not match any repo projects.
							self.matrix_bot.send_to_default(
								&MISMATCHED_PROCESS_FILE.replace(
									"{1}",
									&repo
										.html_url
										.as_ref()
										.context(error::MissingData)?,
								),
							)?;
						}
					} else {
						// Process.toml is invalid.
						self.matrix_bot.send_to_default(
							&MALFORMED_PROCESS_FILE.replace(
								"{1}",
								&repo
									.html_url
									.as_ref()
									.context(error::MissingData)?,
							),
						)?;
					}
				} else {
					// no Process.toml so ignore and continue
				}
			} else {
				log::error!(
				"Error getting projects for repo '{repo_name}'. They may be disabled.",
				repo_name = repo.name
			);
			}
		}
		Ok(())
	}
}
