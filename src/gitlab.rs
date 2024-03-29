use reqwest::header::HeaderMap;
use serde::Deserialize;

use crate::{config::MainConfig, error::Error, types::Result};

impl MainConfig {
	pub fn get_gitlab_api_request_headers(&self) -> Result<HeaderMap> {
		let mut headers = HeaderMap::new();
		headers.insert(
			"PRIVATE-TOKEN",
			self.gitlab_access_token
				.parse()
				.map_err(|_| Error::Message {
					msg: "Couldn't parse Gitlab Access Token as request header"
						.into(),
				})?,
		);
		Ok(headers)
	}
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum GitlabPipelineStatus {
	Created,
	WaitingForResource,
	Preparing,
	Pending,
	Running,
	Scheduled,
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
