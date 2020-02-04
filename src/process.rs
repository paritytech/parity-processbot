use crate::{error, github, Result};
use snafu::{OptionExt, ResultExt};

pub type ProcessInfoMap = std::collections::HashMap<String, ProcessInfo>;

#[derive(serde::Deserialize, serde::Serialize)]
struct ProcessInfoTemp {
	owner: Option<String>,
	delegated_reviewer: Option<String>,
	whitelist: Option<Vec<String>>,
	matrix_room_id: Option<String>,
	backlog: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProcessInfo {
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

pub fn process_from_contents(
	c: github::Contents,
) -> Result<impl Iterator<Item = (String, ProcessInfo)>> {
	base64::decode(&c.content.replace("\n", ""))
		.context(error::Base64)
		.and_then(|s| {
			toml::from_slice::<toml::value::Table>(&s).context(error::Toml)
		})
		.and_then(process_from_table)
		.map(|p| p.into_iter())
}

pub fn process_from_table(tab: toml::value::Table) -> Result<ProcessInfoMap> {
	let temp = tab
		.into_iter()
		.filter_map(|(key, val)| match val {
			toml::value::Value::Table(ref tab) => Some((
				key,
				ProcessInfoTemp {
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
				},
			)),
			_ => None,
		})
		.collect::<Vec<(String, ProcessInfoTemp)>>();
	if temp
		.iter()
		.any(|(_, p)| p.owner.is_none() || p.matrix_room_id.is_none())
	{
		None.context(error::MissingData)
	} else {
		Ok(temp
			.into_iter()
			.map(
				|(
					k,
					ProcessInfoTemp {
						owner,
						delegated_reviewer,
						whitelist,
						matrix_room_id,
						backlog,
					},
				)| {
					(
						k,
						ProcessInfo {
							owner: owner.unwrap(),
							delegated_reviewer,
							whitelist: whitelist.unwrap_or(vec![]),
							matrix_room_id: matrix_room_id.unwrap(),
							backlog,
						},
					)
				},
			)
			.collect::<ProcessInfoMap>())
	}
}
