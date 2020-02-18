use futures_util::future::FutureExt;
use parking_lot::RwLock;
use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
	config::{BotConfig, FeatureConfig},
	constants::*,
	db::*,
	error, github,
	github_bot::GithubBot,
	local_state::*,
	matrix_bot::MatrixBot,
	process, Result,
};

const STATS_MSG: &str = "Organization {org_login}:\n- Repositories with valid Process files: {repos_with_process}\n- Projects in all repositories: {num_projects}\n- Process entries (including owner & matrix room) in all repositories: {num_process}\n- Developers with Github and Matrix handles in BambooHR: {github_to_matrix}\n- Core developers: {core_devs}\n- Open pull requests: {open_prs}\n- Open issues: {open_issues}";

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
		let mut open_prs = 0usize;
		let mut open_issues = 0usize;

		let mut statevec = LocalStateVec::get_or_default(
			&self.db,
			format!("{}", LOCAL_STATE_KEY).into_bytes(),
		)?
		.map(|x| x.map_alive(false));

		log::info!("REPOSITORIES");
		'repo_loop: for repo in repos {
			log::info!("{:?}", repo.name);
			let maybe_process = self
				.github_bot
				.contents(&repo.name, PROCESS_FILE_NAME)
				.await
				.map(|contents| process::process_from_contents(contents).ok())
				.ok()
				.flatten();

			if maybe_process.is_none() {
				continue 'repo_loop;
			}
			let process = maybe_process.unwrap();
			repos_with_process += 1;
			num_process += process.len();

			let projects = self.github_bot.projects(&repo.name).await?;
			if projects.is_empty() {
				log::warn!(
					"Repository '{repo_name}' contains a Process.toml file but no projects",
					repo_name = repo.name,
				);
				continue 'repo_loop;
			}

			let unmatched_process = process
				.iter()
				.filter(|proc| {
					!projects.iter().all(|proj| proj.name != proc.project_name)
				})
				.collect::<Vec<&process::ProcessInfo>>();
			for proc in &unmatched_process {
				log::warn!(
					"'{proc_name}' in Process.toml file doesn't match any projects in repository '{repo_name}'",
                    proc_name = proc.project_name,
					repo_name = repo.name,
				);
			}
			if process.len() == unmatched_process.len() {
				log::warn!(
					"Process.toml file doesn't match any projects in repository '{repo_name}'",
					repo_name = repo.name,
				);
				continue 'repo_loop;
			}

			'issue_loop: for issue in
				self.github_bot.repository_issues(&repo).await?
			{
				let mut local_state = statevec
					.get_entry_or_default(&format!("{}", issue.id).as_bytes());
				local_state.alive = true;

				if issue.pull_request.is_some() {
					// issue is a pull request
					if !self.feature_config.any_pr() {
						continue 'issue_loop;
					}

					// the `mergeable` key is only returned with an individual GET request
					let pr = match self
						.github_bot
						.pull_request(&repo, issue.number)
						.await
					{
						Err(e) => {
							log::error!(
                                "Error getting pull request #{issue_number} in repo {repo_name}: {error}",
                                issue_number = issue.number,
                                repo_name = repo.name,
                                error = e
                            );
							continue 'issue_loop;
						}
						Ok(pr) => pr,
					};

					open_prs += 1;

					let (reviews, issues, status, requested_reviewers) = futures::try_join!(
						self.github_bot.reviews(&pr),
						self.github_bot.linked_issues(
							&repo,
							pr.body.as_ref().context(error::MissingData)?
						),
						self.github_bot.status(&repo.name, &pr),
						self.github_bot.requested_reviewers(&pr)
					)?;
					num_projects += projects.len();

					//
					// CHECK PROJECT / PROCESS
					//
					let project_events = &futures::future::join_all(
						issues.iter().map(|issue| {
							self.github_bot
								.active_project_event(&repo.name, issue.number)
								.map(|x| x.ok().flatten())
						}),
					)
					.await;
					let issue_projects = issues
						.iter()
						.zip(
							Self::projects_from_project_events(
								&project_events,
								&projects,
							)
							.into_iter(),
						)
						.collect::<Vec<(&github::Issue, Option<github::Project>)>>(
						);

					let issue_numbers = std::iter::once(pr.number)
						.chain(issues.iter().map(|issue| issue.number))
						.collect::<Vec<i64>>();
					let combined_process = self
						.combined_process_info(
							&repo,
							&issue_numbers,
							&projects,
							&process,
						)
						.await;
					let pr_project = self
						.check_issue_project(
							&mut local_state,
							&repo,
							&pr,
							&projects,
						)
						.await?;
					if pr_project.is_some() && !combined_process.has_primary() {
						continue 'issue_loop; // PR belongs to a project not listed in Process.toml
					}

					//
					// CHECK MERGE
					//
					match self
						.auto_merge_if_ready(
							&repo,
							&pr,
							&status,
							&combined_process,
							&reviews,
						)
						.await
					{
						Ok(true) => {
							local_state.alive = false;
							continue 'issue_loop; // PR was merged so no more actions
						}
						Err(e) => {
							log::error!(
                                "Error auto-merging pull request #{issue_number} in repo {repo_name}: {error}", 
                                issue_number = pr.number,
                                repo_name = repo.name,
                                error = e,
                            );
						}
						_ => {}
					}

					//
					// CHECK ISSUE ADDRESSED
					//
					if combined_process.is_special(&pr.user.login) {
						// owners and whitelisted devs can open prs without an attached issue.
					} else if issues.is_empty() {
						// author is not special and no issue addressed.
						self.close_for_missing_issue(
							&combined_process,
							&repo,
							&pr,
						)
						.await?;
						local_state.alive = false;
						continue 'issue_loop;
					}

					//
					// CHECK ISSUE ASSIGNED CORRECTLY
					//
					for (issue, maybe_project) in issue_projects {
						if let Some(process_info) = maybe_project
							.and_then(|proj| combined_process.get(&proj.name))
						{
							self.assign_issue_or_warn(
								&mut local_state,
								&process_info,
								&repo,
								&pr,
								&issue,
							)
							.await?;
						} else {
							// project is absent or not in Process.toml
							// so we don't know the owner / matrix room.
						}
					}

					//
					// CHECK REVIEWS
					//
					if let Some(process_info) = pr_project
						.and_then(|proj| combined_process.get(&proj.name))
					{
						self.require_reviewers(
							&mut local_state,
							&pr,
							&process_info,
							&reviews,
							&requested_reviewers,
						)
						.await?;
					} else {
						// project is absent or not in Process.toml
						// so we don't know the owner / matrix room.
					}

					//
					// CHECK STATUS
					//
					self.handle_status(&mut local_state, &pr, &status).await?;
				} else {
					// issue is not a pull request
					if !self.feature_config.any_issue() {
						continue 'issue_loop;
					}

					open_issues += 1;

					//
					// CHECK PROJECT
					//
					self.check_issue_project(
						&mut local_state,
						&repo,
						&issue,
						&projects,
					)
					.await?;
				}
			}
		}

		// delete closed issues / pull requests and persist
		statevec.delete(&self.db, LOCAL_STATE_KEY)?;
		statevec.filter(|x| x.alive).persist(&self.db)?;

		let stats_msg = &STATS_MSG
			.replace("{org_login}", self.github_bot.organization_login())
			.replace("{repos_with_process}", &format!("{}", repos_with_process))
			.replace("{num_projects}", &format!("{}", num_projects))
			.replace("{num_process}", &format!("{}", num_process))
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

	pub async fn combined_process_info(
		&self,
		repo: &github::Repository,
		issue_numbers: &[i64],
		projects: &[github::Project],
		processes: &[process::ProcessInfo],
	) -> process::CombinedProcessInfo {
		process::CombinedProcessInfo::new(process::process_from_projects(
			&Self::projects_from_project_events(
				&futures::future::join_all(issue_numbers.iter().map(|&num| {
					self.github_bot
						.active_project_event(&repo.name, num)
						.map(|x| x.ok().flatten())
				}))
				.await,
				projects,
			),
			processes,
		))
	}

	pub fn projects_from_project_events(
		events: &[Option<github::IssueEvent>],
		projects: &[github::Project],
	) -> Vec<Option<github::Project>> {
		events
			.iter()
			.map(|event| {
				event
					.as_ref()
					.map(|event| event.project_card.as_ref())
					.flatten()
			})
			.flatten()
			.map(|card| {
				projects
					.iter()
					.find(|proj| card.project_id == proj.id)
					.cloned()
			})
			.collect::<_>()
	}
}
