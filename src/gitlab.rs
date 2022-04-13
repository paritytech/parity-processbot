use crate::{error::Error, Result};
use reqwest::header::HeaderMap;
use serde::Deserialize;

pub fn get_request_headers(token: &str) -> Result<HeaderMap> {
	let mut headers = HeaderMap::new();
	headers.insert(
		"PRIVATE-TOKEN",
		token.parse().map_err(|_| Error::Message {
			msg: "Couldn't parse Gitlab Access Token as request header".into(),
		})?,
	);
	Ok(headers)
}

#[derive(Deserialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum GitlabPipelineStatus {
	Pending,
	#[serde(other)]
	Unknown,
}

#[derive(Deserialize, Debug)]
pub struct GitlabJobPipeline {
	pub status: GitlabPipelineStatus,
	pub id: i64,
	pub project_id: i64,
}

#[derive(Deserialize, Debug)]
pub struct GitlabJob {
	pub pipeline: GitlabJobPipeline,
	pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct GitlabPipelineJob {
	pub name: String,
}
