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

const STATS_MSG: &str = "Organization {org_login}:\n- Repositories with valid Process files: {repos_with_process}\n- Projects in all repositories: {num_projects}\n- Process entries (including owner & matrix room) in all repositories: {num_process}\n- Process entries with whitelist: {process_with_whitelist}\n- Whitelisted developers in all repositories: {total_whitelisted}\n- Developers with Github and Matrix handles in BambooHR: {github_to_matrix}\n- Core developers: {core_devs}\n- Open pull requests: {open_prs}\n- Open issues: {open_issues}";

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
			feature_config: FeatureConfig::from_env(),
		}
	}

	pub async fn update(&self) -> Result<()> {
		let repos = self.github_bot.repositories().await?;
		let mut repos_with_process = 0usize;
		let mut num_projects = 0usize;
		let mut num_process = 0usize;
		let mut process_with_whitelist = 0usize;
		let mut total_whitelisted = 0usize;
		let mut open_prs = 0usize;
		let mut open_issues = 0usize;
		for repo in repos.iter() {
			if let Ok(repo_projects) =
				self.github_bot.projects(&repo.name).await
			{
				num_projects += repo_projects.len();

				if let Ok(contents) = self
					.github_bot
					.contents(&repo.name, PROCESS_FILE_NAME)
					.await
				{
					if let Ok(process) =
						process::process_from_contents(contents)
					{
						let pwp =
							projects_with_process(&repo_projects, process);

						num_process += pwp.len();

						for (_proj, name, proc) in pwp.iter() {
							if proc.whitelist.len() > 0 {
								process_with_whitelist += 1;
								total_whitelisted += proc.whitelist.len();
							}

							log::info!("Project {project_name:?} in repository {repo_name:?}:\nowner = {owner:?}\ndelegated_reviewer = {delegate:?}\nwhitelist = {whitelist:?}\nmatrix_room_id = {room_id:?}\nbacklog = {backlog:?}", project_name=name, repo_name=repo.name, owner=proc.owner, delegate=proc.delegated_reviewer, whitelist=proc.whitelist, room_id=proc.matrix_room_id, backlog=proc.backlog);
						}

						let pwp: Vec<(
							Option<github::Project>,
							process::ProcessInfo,
						)> = pwp.into_iter()
							.map(|(proj, _, proc)| (proj, proc))
							.collect();

						if pwp.len() > 0 {
							repos_with_process += 1;

							if self.feature_config.any_issue() {
								let issues = self
									.github_bot
									.repository_issues(&repo)
									.await?;
								open_issues += issues.len();
								for issue in issues {
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
								let prs = self
									.github_bot
									.pull_requests(&repo)
									.await?;
								open_prs += prs.len();
								for pr in prs {
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

		let stats_msg = &STATS_MSG
			.replace("{org_login}", self.github_bot.organization_login())
			.replace("{repos_with_process}", &format!("{}", repos_with_process))
			.replace("{num_projects}", &format!("{}", num_projects))
			.replace("{num_process}", &format!("{}", num_process))
			.replace(
				"{process_with_whitelist}",
				&format!("{}", process_with_whitelist),
			)
			.replace("{total_whitelisted}", &format!("{}", total_whitelisted))
			.replace(
				"{github_to_matrix}",
				&format!("{}", self.github_to_matrix.len()),
			)
			.replace("{core_devs}", &format!("{}", self.core_devs.len()))
			.replace("{open_prs}", &format!("{}", open_prs))
			.replace("{open_issues}", &format!("{}", open_issues));

		log::info!("{}", stats_msg);

		self.matrix_bot
			.send_to_room(&self.config.logs_room_id, stats_msg)?;

		Ok(())
	}
}

fn projects_with_process(
	repo_projects: &[github::Project],
	process: impl Iterator<Item = (String, process::ProcessInfo)>,
) -> Vec<(Option<github::Project>, String, process::ProcessInfo)> {
	process
		.into_iter()
		.map(|(key, process_info): (String, process::ProcessInfo)| {
			(
				repo_projects.iter().find(|rp| rp.name == key).cloned(),
				key,
				process_info,
			)
		})
		.collect()
}
