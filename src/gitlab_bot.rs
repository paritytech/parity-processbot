use crate::error::Error::GitlabApi;
use crate::{error::*, Result};
use curl::easy::Easy;
use serde::Deserialize;
use snafu::ResultExt;

pub struct GitlabBot {
	base_url: String,
	ci_job_name: String,
	private_token: String,
}

#[derive(PartialEq)]
pub enum JobStatus {
	Started,
	AlreadyRunning,
	Finished,
	Unknown,
}

pub struct Job {
	pub status: JobStatus,
	pub status_raw: String,
	pub url: String,
}

#[derive(Deserialize, Debug)]
struct GitlabJob {
	id: i64,
	name: String,
	status: String,
	web_url: String,
}

#[derive(Deserialize, Debug)]
struct Pipeline {
	id: i64,
}

impl GitlabBot {
	pub fn new_with_token(
		hostname: String,
		project: String,
		ci_job_name: String,
		private_token: String,
	) -> Result<Self> {
		let base_url = format!(
			"https://{}/api/v4/projects/{}",
			hostname,
			project.replace("/", "%2F"),
		);

		// This request is just for checking that Gitlab is available and the token is valid.
		get(&base_url, &private_token)?;

		Ok(Self {
			base_url: base_url.to_owned(),
			ci_job_name: ci_job_name.to_owned(),
			private_token: private_token.to_owned(),
		})
	}

	pub fn build_artifact(&self, commit_sha: &str) -> Result<Job> {
		let job = self.fetch_job(commit_sha)?;

		// JobStatus is used by the caller to decide what message to post on Github/Matrix.
		let status = match job.status.to_lowercase().trim() {
			"manual" => JobStatus::Started,
			"running" => JobStatus::AlreadyRunning,
			"success" => JobStatus::Finished,
			"failed" => JobStatus::Finished,
			"canceled" => JobStatus::Finished,
			_ => JobStatus::Unknown,
		};

		if status == JobStatus::Started {
			let play_job_url =
				format!("{}/jobs/{}/play", self.base_url, job.id);
			let response = post(&play_job_url, &self.private_token)?;

			if response.status > 299 {
				return Err(Error::StartingGitlabJobFailed {
					status: response.status,
					url: job.web_url,
					body: response.body,
				});
			}
		}

		Ok(Job {
			status,
			status_raw: job.status,
			url: job.web_url.clone(),
		})
	}

	fn fetch_job(&self, commit_sha: &str) -> Result<GitlabJob> {
		let pipelines_url = format!(
			"{}/pipelines?sha={}&order_by=updated_at&per_page=1",
			&self.base_url, commit_sha,
		);

		let response = get(&pipelines_url, &self.private_token)?;
		let pipelines: Vec<Pipeline> =
			serde_json::from_str(&response.body).context(Json)?;

		if pipelines.is_empty() {
			return Err(Error::GitlabJobNotFound {
				commit_sha: commit_sha.to_string(),
			});
		}

		// If there are multiple pipelines for the same commit, assume the most recently updated
		// one contains the job we want to trigger. Ignore pagination since we are far from having
		// 100 jobs in a single pipeline.
		let jobs_url = format!(
			"{}/pipelines/{}/jobs?per_page=100",
			&self.base_url, pipelines[0].id,
		);

		let response = get(&jobs_url, &self.private_token)?;
		let jobs: Vec<GitlabJob> =
			serde_json::from_str(&response.body).context(Json)?;

		for job in jobs {
			if job.name == self.ci_job_name {
				return Ok(job);
			}
		}

		Err(Error::GitlabJobNotFound {
			commit_sha: commit_sha.to_string(),
		})
	}
}

struct HttpResponse {
	status: u32,
	body: String,
}

// Unlike post(), this returns an Error::GitlabApi for HTTP responses with status code > 299.
// This is because we only want special treatment of these responses for POST jobs/<id>/play,
// where the error returned by the caller should contain the URL to the job on Gitlab
// (aka web_url).
fn get(url: &str, private_token: &str) -> Result<HttpResponse> {
	let mut handle = prepare_handle(url, private_token)?;
	handle.get(false).or_else(map_curl_error)?;

	let response = read_response(&mut handle)?;
	if response.status > 299 {
		return Err(GitlabApi {
			method: "GET".to_string(),
			url: url.to_string(),
			status: response.status,
			body: response.body,
		});
	}
	Ok(response)
}

fn post(url: &str, private_token: &str) -> Result<HttpResponse> {
	let mut handle = prepare_handle(url, private_token)?;
	handle.post(true).or_else(map_curl_error)?;
	read_response(&mut handle)
}

fn prepare_handle(url: &str, private_token: &str) -> Result<Easy> {
	let mut headers = curl::easy::List::new();
	headers
		.append(&format!("Private-Token: {}", private_token))
		.or_else(map_curl_error)?;

	let mut handle = Easy::new();
	handle.http_headers(headers).or_else(map_curl_error)?;
	handle.follow_location(true).or_else(map_curl_error)?;
	handle.max_redirections(2).or_else(map_curl_error)?;
	handle.url(&url).or_else(map_curl_error)?;
	Ok(handle)
}

fn read_response(handle: &mut Easy) -> Result<HttpResponse> {
	let mut dst = Vec::new();
	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|data| {
				dst.extend_from_slice(data);
				Ok(data.len())
			})
			.or_else(map_curl_error)?;
		transfer.perform().or_else(map_curl_error)?;
	}

	let status = handle.response_code().or_else(map_curl_error)?;
	let body = String::from_utf8(dst).context(Utf8)?;

	Ok(HttpResponse { status, body })
}
