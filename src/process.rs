use crate::{
	constants::*, error, github, github_bot::GithubBot, process, Result,
};
use regex::Regex;
use snafu::ResultExt;

pub enum ProcessError {
	ProcessFile,
}

pub async fn get_process(
	github_bot: &GithubBot,
	repo_name: &str,
	issue_number: i64,
) -> Result<CombinedProcessInfo> {
	let process = github_bot
		.contents(&repo_name, PROCESS_FILE_NAME)
		.await
		.and_then(process::process_from_contents)?;

	/*
	// ignore repos with no valid process file
	if maybe_process.is_none() {
		log::warn!(
			"Repository '{repo_name}' has no valid Process.toml file",
			repo_name = repo_name,
		);
		return None;
	}
	*/

	// ignore repos with no projects
	let projects = github_bot.projects(&repo_name).await?;
	/*
	if projects.is_empty() {
		log::warn!(
			"Repository '{repo_name}' contains a Process.toml file but no projects",
			repo_name = repo_name,
		);
		return None;
	}
	*/

	// ignore process entries that do not match a project in the repository
	let (features, process): (Vec<process::ProcessWrapper>, Vec<process::ProcessWrapper>) = process
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
                                    repo_name = repo_name,
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

	let _features = features
		.first()
		.and_then(|f| match f {
			process::ProcessWrapper::Features(feat) => Some(feat.clone()),
			_ => panic!(),
		})
		.unwrap_or(process::ProcessFeatures::default());

	let process = process
		.into_iter()
		.map(|w| match w {
			process::ProcessWrapper::Project(p) => p,
			_ => panic!(),
		})
		.collect::<Vec<process::ProcessInfo>>();

	/*
	// ignore repos with no matching process entries
	if process.is_empty() {
		log::warn!(
			"Process.toml file doesn't match any projects in repository '{repo_name}'",
			repo_name = repo_name,
		);
		return None;
	}
	*/

	combined_process_info(
		github_bot,
		repo_name,
		issue_number,
		&projects,
		&process,
	)
	.await
}

pub async fn combined_process_info(
	github_bot: &GithubBot,
	repo_name: &str,
	number: i64,
	projects: &[github::Project],
	processes: &[process::ProcessInfo],
) -> Result<CombinedProcessInfo> {
	Ok(CombinedProcessInfo(process_from_projects(
		&projects_from_project_events(
			&github_bot.active_project_events(&repo_name, number).await?,
			projects,
		),
		processes,
	)))
}

pub fn projects_from_project_events(
	events: &[github::IssueEvent],
	projects: &[github::Project],
) -> Vec<github::Project> {
	events
		.iter()
		.filter_map(|event| event.project_card.clone())
		.filter_map(|card| {
			projects
				.iter()
				.find(|proj| card.project_id == proj.id)
				.cloned()
		})
		.collect::<_>()
}

pub fn process_from_projects(
	projects: &[github::Project],
	processes: &[ProcessInfo],
) -> Vec<ProcessInfo> {
	projects
		.iter()
		.filter_map(|proj| process_matching_project(processes, proj))
		.cloned()
		.collect::<_>()
}

pub fn process_matching_project<'a>(
	processes: &'a [ProcessInfo],
	project: &github::Project,
) -> Option<&'a ProcessInfo> {
	processes
		.iter()
		.find(|proc| project.name == proc.project_name)
}

#[derive(Clone, Debug)]
pub struct CombinedProcessInfo(Vec<ProcessInfo>);

impl CombinedProcessInfo {
	pub fn len(&self) -> usize {
		self.0.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn iter(&self) -> impl Iterator<Item = &ProcessInfo> {
		self.0.iter()
	}

	pub fn get(&self, project_name: &str) -> Option<&ProcessInfo> {
		self.0.iter().find(|x| x.project_name == project_name)
	}

	pub fn iter_owners(&self) -> impl Iterator<Item = &String> {
		self.0.iter().map(|p| p.owner_or_delegate())
	}

	pub fn iter_room_ids(&self) -> impl Iterator<Item = &String> {
		self.0.iter().map(|p| &p.matrix_room_id)
	}

	pub fn is_owner(&self, login: &str) -> bool {
		self.iter_owners().any(|p| p == login)
	}

	pub fn is_whitelisted(&self, login: &str) -> bool {
		self.0
			.iter()
			.any(|p| p.whitelist.iter().any(|user| user == login))
	}

	pub fn is_special(&self, login: &str) -> bool {
		self.is_owner(login) || self.is_whitelisted(login)
	}
}

pub type ProcessInfoMap = std::collections::HashMap<String, ProcessInfo>;

#[derive(serde::Deserialize, serde::Serialize)]
struct ProcessInfoTemp {
	project_name: String,
	owner: Option<String>,
	delegated_reviewer: Option<String>,
	whitelist: Option<Vec<String>>,
	matrix_room_id: Option<String>,
	backlog: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProcessFeatures {
	pub auto_merge: bool,
	pub issue_project: bool,
	pub issue_addressed: bool,
	pub issue_assigned: bool,
	pub review_requests: bool,
	pub status_notifications: bool,
}

impl Default for ProcessFeatures {
	fn default() -> Self {
		ProcessFeatures {
			auto_merge: true,
			issue_project: false,
			issue_addressed: false,
			issue_assigned: false,
			review_requests: false,
			status_notifications: false,
		}
	}
}

#[derive(Clone, Debug)]
pub enum ProcessWrapper {
	Features(ProcessFeatures),
	Project(ProcessInfo),
}

#[derive(Clone, Debug)]
pub struct ProcessInfo {
	pub project_name: String,
	pub owner: String,
	pub delegated_reviewer: Option<String>,
	pub whitelist: Vec<String>,
	pub matrix_room_id: String,
	pub backlog: Option<String>,
}

impl ProcessInfo {
	pub fn owner_or_delegate(&self) -> &String {
		self.delegated_reviewer.as_ref().unwrap_or(&self.owner)
	}

	/// Checks if the owner of the project matches the login given.
	pub fn is_owner_or_delegate(&self, login: &str) -> bool {
		&self.owner == login
			|| self
				.delegated_reviewer
				.as_ref()
				.map_or(false, |delegate| delegate == login)
	}

	/// Checks if the owner of the project matches the login given.
	pub fn is_owner(&self, login: &str) -> bool {
		&self.owner == login
	}

	/// Checks if the delegated reviewer matches the login given.
	pub fn is_delegated_reviewer(&self, login: &str) -> bool {
		self.delegated_reviewer
			.as_deref()
			.map_or(false, |reviewer| reviewer == login)
	}

	/// Checks that the login is contained within the whitelist.
	pub fn is_whitelisted(&self, login: &str) -> bool {
		self.whitelist.iter().any(|user| user == login)
	}

	pub fn is_special(&self, login: &str) -> bool {
		self.is_owner(login)
			|| self.is_delegated_reviewer(login)
			|| self.is_whitelisted(login)
	}
}

pub fn remove_spaces_from_project_keys(mut s: String) -> String {
	let re = Regex::new(r"(?m)^\[((?:[[:word:]]|[[:punct:]])*)[[:blank:]]")
		.expect("compile regex");
	while re.is_match(&s) {
		s = re.replace_all(&s, "[$1-").to_string();
	}
	s
}

pub fn process_from_contents(
	c: github::Contents,
) -> Result<Vec<ProcessWrapper>> {
	base64::decode(&c.content.replace("\n", ""))
		.context(error::Base64)
		.and_then(|b| {
			let s = remove_spaces_from_project_keys(
				String::from_utf8(b).context(error::Utf8)?,
			);
			toml::from_slice::<toml::value::Table>(s.as_bytes())
				.context(error::Toml)
		})
		.map(process_from_table)
}

pub fn process_from_table(tab: toml::value::Table) -> Vec<ProcessWrapper> {
	tab.into_iter()
		.filter_map(|(key, val)| {
			if key == FEATURES_KEY {
				match val {
					toml::value::Value::Table(ref tab) => {
						Some(ProcessWrapper::Features(ProcessFeatures {
							auto_merge: tab
								.get("auto_merge")
								.and_then(toml::value::Value::as_bool)
								.unwrap_or(true),
							issue_project: tab
								.get("issue_project")
								.and_then(toml::value::Value::as_bool)
								.unwrap_or(false),
							issue_addressed: tab
								.get("issue_addressed")
								.and_then(toml::value::Value::as_bool)
								.unwrap_or(false),
							issue_assigned: tab
								.get("issue_assigned")
								.and_then(toml::value::Value::as_bool)
								.unwrap_or(false),
							review_requests: tab
								.get("review_requests")
								.and_then(toml::value::Value::as_bool)
								.unwrap_or(false),
							status_notifications: tab
								.get("status_notifications")
								.and_then(toml::value::Value::as_bool)
								.unwrap_or(false),
						}))
					}
					_ => None,
				}
			} else {
				match val {
					toml::value::Value::Table(ref tab) => {
						if tab.get("owner").is_none()
							|| tab.get("whitelist").is_none()
							|| tab.get("matrix_room_id").is_none()
						{
							None
						} else {
							Some(ProcessWrapper::Project(ProcessInfo {
								project_name: key,
								owner: tab
									.get("owner")
									.and_then(toml::value::Value::as_str)
									.map(str::to_owned)
									.unwrap(),
								delegated_reviewer: tab
									.get("delegated_reviewer")
									.and_then(toml::value::Value::as_str)
									.map(str::to_owned),
								whitelist: tab
									.get("whitelist")
									.and_then(toml::value::Value::as_array)
									.map(|a| {
										a.iter()
											.filter_map(
												toml::value::Value::as_str,
											)
											.map(str::to_owned)
											.collect::<Vec<String>>()
									})
									.unwrap_or(vec![]),
								matrix_room_id: tab
									.get("matrix_room_id")
									.and_then(toml::value::Value::as_str)
									.map(str::to_owned)
									.unwrap(),
								backlog: tab
									.get("backlog")
									.and_then(toml::value::Value::as_str)
									.map(str::to_owned),
							}))
						}
					}
					_ => None,
				}
			}
		})
		.collect::<_>()
}
