use parking_lot::RwLock;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
	config::{BotConfig, FeatureConfig},
	constants::*,
	error, github,
	github_bot::GithubBot,
	matrix_bot::MatrixBot,
	process, Result,
};

pub struct Bot {
	pub db: Arc<RwLock<DB>>,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub core_devs: Vec<github::User>,
	pub github_to_matrix: HashMap<String, String>,
	pub config: BotConfig,
	pub feature_config: FeatureConfig,
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
			feature_config: dbg!(FeatureConfig::from_env()),
		}
	}

	pub async fn update(&self) -> Result<()> {
		if self.feature_config.any() {
			for repo in self.github_bot.repositories().await?.iter() {
				if let Ok(repo_projects) =
					self.github_bot.projects(&repo.name).await
				{
					if let Ok(contents) = self
						.github_bot
						.contents(&repo.name, "Process.toml")
						.await
					{
						if let Ok(process) =
							process::process_from_contents(contents)
						{
							// projects in Process.toml are useless if they do not match a project
							// in the repo
							let pwp =
								projects_with_process(&repo_projects, process);

							if pwp.len() > 0 {
								if self.feature_config.any_issue() {
									for issue in self
										.github_bot
										.repository_issues(&repo)
										.await?
									{
										// if issue.pull_request.is_some() then this issue is a pull
										// request, which we treat differently
										if issue.pull_request.is_none() {
											match self
												.handle_issue(
													pwp.as_ref(),
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
								}

								if self.feature_config.any_pr() {
									for pr in self
										.github_bot
										.pull_requests(&repo)
										.await?
									{
										match self
											.handle_pull_request(
												pwp.as_ref(),
												&repo,
												&pr,
											)
											.await
										{
											Err(e) => {
												log::error!(
                                                    "Error handling pull request #{issue_number} in repo {repo_name}: {error}",
                                                    issue_number = pr.number,
                                                    repo_name = repo.name,
                                                    error = e
                                                );
											}
											_ => {}
										}
									}
								}
							} else {
								// Process.toml does not match any repo projects.
								self.matrix_bot.send_to_default(
									&MISMATCHED_PROCESS_FILE.replace(
										"{repo_url}",
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
									"{repo_url}",
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
		}
		Ok(())
	}
}

fn projects_with_process(
	repo_projects: &[github::Project],
	process: impl Iterator<Item = (String, process::ProcessInfo)>,
) -> Vec<(Option<github::Project>, process::ProcessInfo)> {
	process
		.into_iter()
		.map(|(key, process_info): (String, process::ProcessInfo)| {
			(
				repo_projects.iter().find(|rp| rp.name == key).cloned(),
				process_info,
			)
		})
		.collect::<Vec<(Option<github::Project>, process::ProcessInfo)>>()
}
