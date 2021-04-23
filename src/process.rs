use crate::{
	constants::*, error, github, github_bot::GithubBot, process, Result,
};
use serde::Deserialize;
use snafu::ResultExt;

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

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ProcessInfo {
	pub project_name: String,
	pub owner: String,
	pub delegated_reviewer: Option<String>,
	#[serde(default)]
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

pub async fn get_process(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	issue_number: i64,
) -> Result<(CombinedProcessInfo, Vec<String>)> {
	// get Process file from master
	let process = github_bot
		.contents(owner, repo_name, PROCESS_FILE, "master")
		.await
		.and_then(process::process_from_contents)?;

	// repos with no projects can have no valid process info
	let projects = github_bot.projects(owner, repo_name).await?;

	// ignore process entries that do not match a project in the repository
	let mut warnings: Vec<String> = vec![];
	let process = process
		.into_iter()
		.filter(|p| {
			let keep = projects.iter().any(|pj| pj.name == p.project_name);
			if !keep {
				let warning = format!(
					"'{}' does not match any projects in {}'s {}",
					p.project_name, repo_name, PROCESS_FILE
				);
				log::info!("{}", &warning);
				warnings.push(warning);
			}
			keep
		})
		.collect::<Vec<ProcessInfo>>();

	combined_process_info(
		github_bot,
		owner,
		repo_name,
		issue_number,
		&projects,
		&process,
	)
	.await
	.map(|info| (info, warnings))
}

fn process_from_contents(c: github::Contents) -> Result<Vec<ProcessInfo>> {
	base64::decode(&c.content.replace("\n", ""))
		.context(error::Base64)
		.and_then(|b| {
			let s = String::from_utf8(b).context(error::Utf8)?;
			serde_json::from_str(&s).context(error::Json)
		})
}

/// Return a CombinedProcessInfo struct representing together each process entry that matches a
/// project in the repo.
async fn combined_process_info(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	number: i64,
	projects: &[github::Project],
	processes: &[process::ProcessInfo],
) -> Result<CombinedProcessInfo> {
	/// Return process entries matching the given projects, in the order of the projects.
	fn process_matching_projects(
		processes: &[ProcessInfo],
		projects: &[github::Project],
	) -> Vec<ProcessInfo> {
		/// Return the process entry matching a given project in the repo.
		fn process_matching_project<'a>(
			processes: &'a [ProcessInfo],
			project: &github::Project,
		) -> Option<&'a ProcessInfo> {
			processes.iter().find(|p| project.name == p.project_name)
		}

		projects
			.iter()
			.filter_map(|pj| process_matching_project(processes, pj))
			.cloned()
			.collect::<_>()
	}

	fn projects_matching_project_events(
		events: &[github::IssueEvent],
		projects: &[github::Project],
	) -> Vec<github::Project> {
		events
			.iter()
			.filter_map(|event| event.project_card.clone())
			.filter_map(|card| {
				projects.iter().find(|pj| card.project_id == pj.id).cloned()
			})
			.collect::<_>()
	}

	Ok(CombinedProcessInfo(process_matching_projects(
		processes,
		&projects_matching_project_events(
			&github_bot
				.active_project_events(owner, repo_name, number)
				.await?,
			projects,
		),
	)))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_process_json() {
		serde_json::from_str::<Vec<ProcessInfo>>(include_str!(
			"../Process.json"
		))
		.unwrap();
	}
}
