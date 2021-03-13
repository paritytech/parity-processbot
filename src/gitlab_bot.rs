use crate::{error::*, Result};
use curl::easy::Easy;
use serde::Deserialize;
use url::Url;

pub struct GitlabBot {
	urls: UrlBuilder,
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

#[derive(Deserialize, Debug, Clone)]
struct Pipeline {
	id: i64,
}

impl GitlabBot {
	pub fn new_with_token(
		hostname: &str,
		project: &str,
		ci_job_name: &str,
		private_token: &str,
	) -> Result<Self> {
		let urls = UrlBuilder::new(hostname, project)?;

		// This request is just for checking that Gitlab is available and the token is valid.
		let project_url = urls.project_url()?;
		get(&project_url, &private_token)?;

		Ok(Self {
			urls,
			ci_job_name: ci_job_name.to_owned(),
			private_token: private_token.to_owned(),
		})
	}

	pub fn new_placeholder_for_testing() -> Self {
		Self {
			urls: UrlBuilder::new_placeholder_for_testing(),
			ci_job_name: "".to_string(),
			private_token: "".to_string(),
		}
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
			let play_job_url = self.urls.play_job_url(job.id)?;
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
			url: job.web_url,
		})
	}

	fn fetch_job(&self, commit_sha: &str) -> Result<GitlabJob> {
		let pipeline = self.fetch_pipeline_for_commit(commit_sha)?;
		let jobs = self.fetch_jobs_for_pipeline(pipeline.id)?;

		for job in jobs {
			if job.name == self.ci_job_name {
				return Ok(job);
			}
		}

		Err(Error::GitlabJobNotFound {
			commit_sha: commit_sha.to_string(),
		})
	}

	fn fetch_jobs_for_pipeline(
		&self,
		pipeline_id: i64,
	) -> Result<Vec<GitlabJob>> {
		let jobs_url = self.urls.jobs_url_for_pipeline(pipeline_id)?;
		let response = get(&jobs_url, &self.private_token)?;

		let jobs: Vec<GitlabJob> = serde_json::from_str(&response.body)
			.or_else(|e| Err(Error::Json { source: e }))?;

		Ok(jobs)
	}

	fn fetch_pipeline_for_commit(&self, commit_sha: &str) -> Result<Pipeline> {
		let pipelines_url = self.urls.pipelines_url_for_commit(commit_sha)?;
		let response = get(&pipelines_url, &self.private_token)?;

		let pipelines: Vec<Pipeline> = serde_json::from_str(&response.body)
			.or_else(|e| Err(Error::Json { source: e }))?;

		if pipelines.is_empty() {
			return Err(Error::GitlabJobNotFound {
				commit_sha: commit_sha.to_string(),
			});
		}

		Ok(pipelines[0].clone())
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
fn get(url: &Url, private_token: &str) -> Result<HttpResponse> {
	let mut handle = prepare_handle(url, private_token)?;
	handle.get(false)?;

	let response = read_response(&mut handle)?;
	if response.status > 299 {
		return Err(Error::GitlabApi {
			method: "GET".to_string(),
			url: url.to_string(),
			status: response.status,
			body: response.body,
		});
	}
	Ok(response)
}

fn post(url: &Url, private_token: &str) -> Result<HttpResponse> {
	let mut handle = prepare_handle(url, private_token)?;
	handle.post(true)?;
	read_response(&mut handle)
}

fn prepare_handle(url: &Url, private_token: &str) -> Result<Easy> {
	let mut headers = curl::easy::List::new();
	headers.append(&format!("Private-Token: {}", private_token))?;

	let mut handle = Easy::new();
	handle.http_headers(headers)?;
	handle.follow_location(true)?;
	handle.max_redirections(2)?;
	handle.url(&url.to_string())?;
	Ok(handle)
}

fn read_response(handle: &mut Easy) -> Result<HttpResponse> {
	let mut dst = Vec::new();
	{
		let mut transfer = handle.transfer();
		transfer.write_function(|data| {
			dst.extend_from_slice(data);
			Ok(data.len())
		})?;
		transfer.perform()?;
	}

	let status = handle.response_code()?;
	let body =
		String::from_utf8(dst).or_else(|e| Err(Error::Utf8 { source: e }))?;

	Ok(HttpResponse { status, body })
}

struct UrlBuilder {
	base_url: Url,
	base_path: Vec<String>,
}

impl UrlBuilder {
	pub fn new(hostname: &str, project: &str) -> Result<Self> {
		let base_url_str = format!("https://{}", hostname);

		let base_url = Url::parse(&base_url_str)
			.or_else(|e| Err(Error::ParseUrl { source: e }))?;

		let base_path = vec!["api", "v4", "projects", &project]
			.into_iter()
			.map(|s| s.to_string())
			.collect();

		Ok(Self {
			base_url,
			base_path,
		})
	}

	pub fn new_placeholder_for_testing() -> Self {
		Self {
			base_url: Url::parse("http://foo.com").unwrap(),
			base_path: vec![],
		}
	}

	pub fn project_url(&self) -> Result<Url> {
		let mut project_url = self.base_url.clone();

		{
			let mut path_segments =
				project_url.path_segments_mut().or_else(|()| {
					Err(Error::UrlCannotBeBase {
						url: self.base_url.to_string(),
					})
				})?;
			path_segments.extend(&self.base_path);
		}

		Ok(project_url)
	}

	pub fn pipelines_url_for_commit(&self, commit_sha: &str) -> Result<Url> {
		let mut pipelines_url = self.base_url.clone();

		{
			let mut path_segments =
				pipelines_url.path_segments_mut().or_else(|()| {
					Err(Error::UrlCannotBeBase {
						url: self.base_url.to_string(),
					})
				})?;
			path_segments.extend(&self.base_path);
			path_segments.push("pipelines");
		}

		// If there are multiple pipelines for the same commit, assume the most recently updated
		// one contains the job we want to trigger.
		pipelines_url
			.query_pairs_mut()
			.clear()
			.append_pair("sha", commit_sha)
			.append_pair("order_by", "updated_at")
			.append_pair("per_page", "1");

		Ok(pipelines_url)
	}

	pub fn jobs_url_for_pipeline(&self, pipeline_id: i64) -> Result<Url> {
		let mut jobs_url = self.base_url.clone();

		{
			let mut path_segments =
				jobs_url.path_segments_mut().or_else(|()| {
					Err(Error::UrlCannotBeBase {
						url: self.base_url.to_string(),
					})
				})?;

			path_segments.extend(&self.base_path);
			path_segments.extend(&[
				"pipelines",
				&pipeline_id.to_string(),
				"jobs",
			]);
		}

		Ok(jobs_url)
	}

	pub fn play_job_url(&self, job_id: i64) -> Result<Url> {
		let mut play_job_url = self.base_url.clone();

		{
			let mut path_segments =
				play_job_url.path_segments_mut().or_else(|()| {
					Err(Error::UrlCannotBeBase {
						url: self.base_url.to_string(),
					})
				})?;

			path_segments.extend(&self.base_path);
			path_segments.extend(&["jobs", &job_id.to_string(), "play"]);
		}

		Ok(play_job_url)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn builder() -> UrlBuilder {
		UrlBuilder::new("gitlab.parity.io", "parity/processbot-test-repo")
			.unwrap()
	}

	fn assert_url(url: Result<Url>, expected: &str) {
		assert!(url.is_ok());
		assert_eq!(&url.unwrap().to_string(), expected);
	}

	#[test]
	fn test_project_url() {
		let project_url = builder().project_url();

		assert_url(
			project_url,
			"https://gitlab.parity.io/api/v4/projects/parity%2Fprocessbot-test-repo"
		);
	}

	#[test]
	fn test_pipelines_url_for_commit() {
		let pipelines_url = builder().pipelines_url_for_commit(
			"3194e877da3bb6e28df5c0a0d27abcf31b11ba60",
		);

		assert_url(
			pipelines_url,
			"https://gitlab.parity.io/api/v4/projects/parity%2Fprocessbot-test-repo/pipelines?sha=3194e877da3bb6e28df5c0a0d27abcf31b11ba60&order_by=updated_at&per_page=1"
		);
	}

	#[test]
	fn test_jobs_url_for_pipeline() {
		let jobs_url = builder().jobs_url_for_pipeline(42);

		assert_url(
			jobs_url,
			"https://gitlab.parity.io/api/v4/projects/parity%2Fprocessbot-test-repo/pipelines/42/jobs"
		);
	}

	#[test]
	fn test_play_jobs_url() {
		let jobs_url = builder().play_job_url(23);

		assert_url(
			jobs_url,
			"https://gitlab.parity.io/api/v4/projects/parity%2Fprocessbot-test-repo/jobs/23/play"
		);
	}
}
