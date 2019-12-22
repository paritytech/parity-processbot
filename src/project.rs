#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Projects(pub std::collections::HashMap<String, ProjectInfo>);

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ProjectInfo {
	pub owner: Option<String>,
	pub delegated_reviewer: Option<String>,
	pub whitelist: Option<Vec<String>>,
	pub matrix_room_id: Option<String>,
}

impl From<toml::value::Table> for Projects {
	fn from(tab: toml::value::Table) -> Projects {
		Projects(
			tab.into_iter()
				.filter_map(|(key, val)| match val {
					toml::value::Value::Table(ref tab) => Some((
						key,
						ProjectInfo {
							owner: val
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
						},
					)),
					_ => None,
				})
				.collect(),
		)
	}
}
