use futures_util::future::FutureExt;
//use parking_lot::RwLock;
//use rocksdb::DB;
use snafu::OptionExt;
use std::collections::HashMap;
//use std::sync::Arc;

use crate::{
	config::BotConfig, constants::*, error, github, github_bot::GithubBot,
	matrix_bot::MatrixBot, process, Result,
};

const STATS_MSG: &str = "ORGANIZATION {org_login}\nRepositories with valid Process files: {repos_with_process}\nProjects in all repositories: {num_projects}\nProcess entries (including owner & matrix room) in all repositories: {num_process}\nDevelopers with Github and Matrix handles in BambooHR: {github_to_matrix}\nCore developers: {core_devs}\nOpen pull requests: {open_prs}\nOpen issues: {open_issues}";

pub struct Bot {
	//	pub db: Arc<RwLock<DB>>,
	pub github_bot: GithubBot,
	pub matrix_bot: MatrixBot,
	pub core_devs: Vec<github::User>,
	pub github_to_matrix: HashMap<String, String>,
	pub config: BotConfig,
}

impl Bot {
	pub fn new(
		//		db: Arc<RwLock<DB>>,
		github_bot: GithubBot,
		matrix_bot: MatrixBot,
		core_devs: Vec<github::User>,
		github_to_matrix: HashMap<String, String>,
	) -> Bot {
		Bot {
			//			db,
			github_bot,
			matrix_bot,
			core_devs,
			github_to_matrix,
			config: BotConfig::from_env(),
		}
	}

	pub async fn update(&self) -> Result<()> {
		let repos = self
			.github_bot
			.installation_repositories()
			.await?
			.repositories;
		let mut repos_with_process = 0usize;
		let mut num_projects = 0usize;
		let mut num_process = 0usize;
		let mut open_prs = 0usize;
		let mut open_issues = 0usize;

		//		let mut statevec = LocalStateVec::get_or_default(
		//			&self.db,
		//			format!("{}", LOCAL_STATE_KEY).into_bytes(),
		//		)?
		//		.map(|x| x.map_alive(false));

		log::info!("REPOSITORIES");
		'repo_loop: for repo in repos {
			log::info!("{:?}", repo.name);
			let maybe_process = self
				.github_bot
				.contents(&repo.name, PROCESS_FILE_NAME)
				.await
				.map(|contents| {
					let proc = process::process_from_contents(contents);
					if proc.is_err() {
						log::debug!("{:#?}", proc);
					}
					proc.ok()
				})
				.ok()
				.flatten();

			// ignore repos with no valid process file
			if maybe_process.is_none() {
				log::warn!(
					"Repository '{repo_name}' has no valid Process.toml file",
					repo_name = repo.name,
				);
				continue 'repo_loop;
			}

			// ignore repos with no projects
			let projects = self.github_bot.projects(&repo.name).await?;
			if projects.is_empty() {
				log::warn!(
					"Repository '{repo_name}' contains a Process.toml file but no projects",
					repo_name = repo.name,
				);
				continue 'repo_loop;
			}

			// ignore process entries that do not match a project in the repository
			let (features, process): (Vec<process::ProcessWrapper>, Vec<process::ProcessWrapper>) = maybe_process.unwrap()
				.into_iter()
				.filter(|proc| {
                    match proc {
                        process::ProcessWrapper::Features(_) => true,
                        process::ProcessWrapper::Project(proc) => {
                            let keep = projects.iter().any(|proj| proj.name.replace(" ", "-") == proc.project_name);
                            if !keep {
                                log::warn!(
                                    "'{proc_name}' in Process.toml file doesn't match any projects in repository '{repo_name}'",
                                    proc_name = proc.project_name,
                                    repo_name = repo.name,
                                );
                            }
                            keep
                        }
                    }
				})
                .partition(|proc| match proc {
                    process::ProcessWrapper::Features(_) => true,
                    process::ProcessWrapper::Project(_) => false,
                });
			let features = features
				.first()
				.and_then(|f| match f {
					process::ProcessWrapper::Features(feat) => {
						Some(feat.clone())
					}
					_ => None,
				})
				.unwrap_or(process::ProcessFeatures::default());
			let process = process
				.into_iter()
				.map(|w| match w {
					process::ProcessWrapper::Project(p) => p,
					_ => panic!(),
				})
				.collect::<Vec<process::ProcessInfo>>();

			// ignore repos with no matching process entries
			if process.is_empty() {
				log::warn!(
					"Process.toml file doesn't match any projects in repository '{repo_name}'",
					repo_name = repo.name,
				);
				continue 'repo_loop;
			}

			repos_with_process += 1;
			num_process += process.len();

			'issue_loop: for issue in
				self.github_bot.repository_issues(&repo).await?
			{
				//				let mut local_state = statevec
				//					.get_entry_or_default(&format!("{}", issue.id).as_bytes());
				//				local_state.alive = true;

				//				let author_is_core =
				//					self.core_devs.iter().any(|u| u.id == issue.user.id);

				open_issues += 1;

				if issue.pull_request.is_some() {
					// issue is a pull request

					// the `mergeable` key is only returned with an individual GET request
					let pr = match self
						.github_bot
						.pull_request(&repo.name, issue.number)
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

					let (reviews, issues, status, _requested_reviewers) = futures::try_join!(
						self.github_bot.reviews(&pr),
						self.github_bot.linked_issues(
							&repo.name,
							pr.body.as_ref().context(error::MissingData)?
						),
						self.github_bot.status(&repo.name, &pr.head.sha),
						self.github_bot.requested_reviewers(&pr)
					)?;
					num_projects += projects.len();

					//
					// CHECK PROJECT / PROCESS
					//
					let issue_numbers = std::iter::once(pr.number)
						.chain(issues.iter().map(|issue| issue.number))
						.collect::<Vec<i64>>();
					let combined_process = process::combined_process_info(
						&self.github_bot,
						&repo,
						&issue_numbers,
						&projects,
						&process,
					)
					.await;

					let pr_project = self
						.github_bot
						.issue_project(&repo.name, pr.number, &projects)
						.await;

					// ignore issue attached to project not listed in process file
					if pr_project.is_some() && !combined_process.has_primary() {
						continue 'issue_loop;
					}

					// if the issue has no project but:
					// - there is only one project in the repo, OR
					// - the author is owner/whitelisted on only one.
					// then attach that project
					if features.issue_project {
						if pr_project.is_none() {
							//							self.try_attach_project(
							//								&mut local_state,
							//								&repo,
							//								&pr,
							//								&projects,
							//								&process,
							//								author_is_core,
							//							)
							//							.await?;
						}
					}

					//
					// CHECK MERGE
					//
					if features.auto_merge {
						/*
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
								//								local_state.alive = false;
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
						*/
					}

					//
					// SHORT CIRCUIT
					//
					continue 'issue_loop; // TODO: remove

				/*
				//
				// CHECK ISSUE ADDRESSED
				//
				if features.issue_addressed {
					if combined_process.is_special(&pr.user.login) {
						// owners and whitelisted devs can open prs without an attached issue.
					} else if issues.is_empty() {
						// author is not special and no issue addressed.
						//							self.pr_missing_issue(&mut local_state, &repo, &pr)
						//								.await?;
					}
				}

				//
				// CHECK ISSUE ASSIGNED CORRECTLY
				//
				if features.issue_assigned {
					for (_issue, maybe_project) in
						self.issue_projects(&repo, &issues, &projects).await
					{
						if let Some(_process_info) =
							maybe_project.and_then(|proj| {
								combined_process.get(&proj.name)
							}) {
							//								self.assign_issue_or_warn(
							//									&mut local_state,
							//									&process_info,
							//									&repo,
							//									&pr,
							//									&issue,
							//								)
							//								.await?;
						} else {
							// project is absent or not in Process.toml
							// so we don't know the owner / matrix room.
						}
					}
				}

				//
				// CHECK REVIEWS
				//
				if features.review_requests {
					if let Some(_process_info) = pr_project
						.and_then(|proj| combined_process.get(&proj.name))
					{
						//							self.require_reviewers(
						//								&mut local_state,
						//								&pr,
						//								&process_info,
						//								&reviews,
						//								&requested_reviewers,
						//							)
						//							.await?;
					} else {
						// project is absent or not in Process.toml
						// so we don't know the owner / matrix room.
					}
				}

				//
				// CHECK STATUS
				//
				if features.status_notifications {
					//						self.handle_status(&mut local_state, &pr, &status)
					//							.await?;
				}
				*/
				} else {
					/*
					let issue = match self
						.github_bot
						.issue(&repo, issue.number)
						.await
					{
						Err(e) => {
							log::error!(
								"Error getting issue #{issue_number} in repo {repo_name}: {error}",
								issue_number = issue.number,
								repo_name = repo.name,
								error = e
							);
							continue 'issue_loop;
						}
						Ok(issue) => issue,
					};

					// issue is not a pull request
					open_issues += 1;

					//
					// CHECK PROJECT
					//
					let issue_project = self
						.issue_project(&repo.name, issue.number, &projects)
						.await;

					// ignore issue attached to project not listed in process file
					if issue_project.is_some()
						&& process::process_matching_project(
							&process,
							issue_project.unwrap(),
						)
						.is_some()
					{
						continue 'issue_loop;
					}

					// if the issue has no project but:
					// - there is only one project in the repo, OR
					// - the author is owner/whitelisted on only one.
					// then attach that project
					if features.issue_project {
						if issue_project.is_none() {
							//							self.try_attach_project(
							//								&mut local_state,
							//								&repo,
							//								&issue,
							//								&projects,
							//								&process,
							//								author_is_core,
							//							)
							//							.await?;
						}
					}
					*/
				}
			}
		}

		// delete closed issues / pull requests and persist
		//		statevec.delete(&self.db, LOCAL_STATE_KEY)?;
		//		statevec.filter(|x| x.alive).persist(&self.db)?;

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
			.replace("{open_issues}", &format!("{}", open_issues - open_prs));

		log::info!("{}", stats_msg);

		//		self.matrix_bot
		//			.send_to_room(&self.config.logs_room_id, stats_msg)?;

		Ok(())
	}

	pub async fn issue_projects<'a>(
		&self,
		repo: &github::Repository,
		issues: &'a [github::Issue],
		projects: &[github::Project],
	) -> Vec<(&'a github::Issue, Option<github::Project>)> {
		issues
			.iter()
			.zip(
				process::projects_from_project_events(
					&futures::future::join_all(issues.iter().map(|issue| {
						self.github_bot
							.active_project_event(&repo.name, issue.number)
							.map(|x| x.ok().flatten())
					}))
					.await,
					&projects,
				)
				.into_iter(),
			)
			.collect::<_>()
	}
}
