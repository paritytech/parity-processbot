use crate::{error, github, Result};
use snafu::{OptionExt, ResultExt};

#[derive(Clone, Debug)]
pub struct CombinedProcessInfo {
	pub primary: Option<ProcessInfo>,
	pub linked: Vec<ProcessInfo>,
}

impl CombinedProcessInfo {
	pub fn new(mut v: Vec<Option<ProcessInfo>>) -> Self {
		let linked = v.split_off(1);
		Self {
			primary: v.first().cloned().flatten(),
			linked: linked.into_iter().filter_map(|x| x).collect::<_>(),
		}
	}

	pub fn len(&self) -> usize {
		self.primary.iter().len() + self.linked.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn has_primary(&self) -> bool {
		self.primary.is_some()
	}

	pub fn has_linked(&self) -> bool {
		!self.linked.is_empty()
	}

	pub fn iter(&self) -> impl Iterator<Item = &ProcessInfo> {
		self.primary.iter().chain(self.linked.iter())
	}

	pub fn get(&self, project_name: &str) -> Option<&ProcessInfo> {
		self.primary
			.iter()
			.chain(self.linked.iter())
			.find(|x| x.project_name == project_name)
	}

	pub fn primary_owner(&self) -> Option<String> {
		self.primary
			.as_ref()
			.map(|p| p.delegated_reviewer.as_ref().unwrap_or(&p.owner))
			.cloned()
	}

	pub fn iter_linked_owners(&self) -> impl Iterator<Item = &String> {
		self.linked.iter().map(|p| p.owner_or_delegate())
	}

	pub fn iter_owners(&self) -> impl Iterator<Item = &String> {
		self.primary
			.iter()
			.chain(self.linked.iter())
			.map(|p| p.owner_or_delegate())
	}

	pub fn primary_room_id(&self) -> Option<String> {
		self.primary.as_ref().map(|p| p.matrix_room_id.clone())
	}

	pub fn iter_linked_room_ids(&self) -> impl Iterator<Item = &String> {
		self.linked.iter().map(|p| &p.matrix_room_id)
	}

	pub fn iter_room_ids(&self) -> impl Iterator<Item = &String> {
		self.primary
			.iter()
			.chain(self.linked.iter())
			.map(|p| &p.matrix_room_id)
	}

	pub fn is_primary_owner(&self, login: &str) -> bool {
		self.primary_owner().map_or(false, |owner| owner == login)
	}

	pub fn is_linked_owner(&self, login: &str) -> bool {
		self.iter_linked_owners().any(|owner| owner == login)
	}

	pub fn is_owner(&self, login: &str) -> bool {
		self.iter_owners().any(|p| p == login)
	}

	pub fn is_primary_whitelisted(&self, login: &str) -> bool {
		self.primary
			.as_ref()
			.map_or(false, |p| p.whitelist.iter().any(|user| user == login))
	}

	pub fn is_linked_whitelisted(&self, login: &str) -> bool {
		self.linked
			.iter()
			.any(|p| p.whitelist.iter().any(|user| user == login))
	}

	pub fn is_whitelisted(&self, login: &str) -> bool {
		self.primary
			.iter()
			.chain(self.linked.iter())
			.any(|p| p.whitelist.iter().any(|user| user == login))
	}

	pub fn is_primary_special(&self, login: &str) -> bool {
		self.is_primary_owner(login) || self.is_primary_whitelisted(login)
	}

	pub fn is_linked_special(&self, login: &str) -> bool {
		self.is_linked_owner(login) || self.is_linked_whitelisted(login)
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
pub struct ProcessInfo {
	pub project_name: String,
	pub owner: String,
	pub delegated_reviewer: Option<String>,
	pub whitelist: Vec<String>,
	pub matrix_room_id: String,
	pub backlog: Option<String>,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct AuthorInfo {
	pub is_owner_or_delegate: bool,
	pub is_whitelisted: bool,
}

impl AuthorInfo {
	pub fn is_special(&self) -> bool {
		self.is_owner_or_delegate || self.is_whitelisted
	}
}

impl ProcessInfo {
	pub fn owner_or_delegate(&self) -> &String {
		self.delegated_reviewer.as_ref().unwrap_or(&self.owner)
	}

	pub fn author_info(&self, login: &str) -> AuthorInfo {
		let is_owner = self.is_owner(login);
		let is_delegated_reviewer = self.is_delegated_reviewer(login);
		let is_whitelisted = self.is_whitelisted(login);

		AuthorInfo {
			is_owner_or_delegate: is_owner || is_delegated_reviewer,
			is_whitelisted,
		}
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

pub fn process_from_projects(
	projects: &[Option<github::Project>],
	processes: &[ProcessInfo],
) -> Vec<Option<ProcessInfo>> {
	projects
		.iter()
		.map(|proj| {
			processes
				.iter()
				.find(|proc| {
					proj.as_ref().map_or(false, |p| p.name == proc.project_name)
				})
				.cloned()
		})
		.collect::<_>()
}

pub fn process_from_contents(c: github::Contents) -> Result<Vec<ProcessInfo>> {
	base64::decode(&c.content.replace("\n", ""))
		.context(error::Base64)
		.and_then(|s| {
			toml::from_slice::<toml::value::Table>(&s).context(error::Toml)
		})
		.and_then(process_from_table)
}

pub fn process_from_table(tab: toml::value::Table) -> Result<Vec<ProcessInfo>> {
	let temp = tab
		.into_iter()
		.filter_map(|(key, val)| match val {
			toml::value::Value::Table(ref tab) => Some(ProcessInfoTemp {
				project_name: key,
				owner: tab
					.get("owner")
					.and_then(toml::value::Value::as_str)
					.map(str::to_owned),
				delegated_reviewer: tab
					.get("delegated_reviewer")
					.and_then(toml::value::Value::as_str)
					.map(str::to_owned),
				whitelist: tab
					.get("whitelist")
					.and_then(toml::value::Value::as_array)
					.map(|a| {
						a.iter()
							.filter_map(toml::value::Value::as_str)
							.map(str::to_owned)
							.collect::<Vec<String>>()
					}),
				matrix_room_id: tab
					.get("matrix_room_id")
					.and_then(toml::value::Value::as_str)
					.map(str::to_owned),
				backlog: tab
					.get("backlog")
					.and_then(toml::value::Value::as_str)
					.map(str::to_owned),
			}),
			_ => None,
		})
		.collect::<Vec<ProcessInfoTemp>>();
	if temp
		.iter()
		.any(|p| p.owner.is_none() || p.matrix_room_id.is_none())
	{
		None.context(error::MissingData)
	} else {
		Ok(temp
			.into_iter()
			.map(
				|ProcessInfoTemp {
				     project_name,
				     owner,
				     delegated_reviewer,
				     whitelist,
				     matrix_room_id,
				     backlog,
				 }| {
					ProcessInfo {
						project_name,
						owner: owner.unwrap(),
						delegated_reviewer,
						whitelist: whitelist.unwrap_or(vec![]),
						matrix_room_id: matrix_room_id.unwrap(),
						backlog,
					}
				},
			)
			.collect::<Vec<ProcessInfo>>())
	}
}
