use std::borrow::Cow;
use std::collections::HashMap;
use std::time::SystemTime;

use byteorder::{BigEndian, ByteOrder};
use futures::future::Future;
use gm::room::{NewRoom, RoomExt};
use gm::types::replies::RoomCreationOptions;
use gm::types::room::Room;
use gm::MatrixClient;
use hyperx::header::TypedHeaders;
use rocksdb::{IteratorMode, DB};
use serde::*;
use snafu::ResultExt;

use crate::{error, github, Result};

/// Maps the response into an error if it's not a success.
fn map_response_status(mut val: reqwest::Response) -> Result<reqwest::Response> {
	if val.status().is_success() {
		Ok(val)
	} else {
		Err(error::Error::Response {
			status: val.status(),
			body: val.json().context(error::Http)?,
		})
	}
}

#[derive(Serialize, Deserialize, Debug)]
enum DbEntry {
	PullRequest {
		created_at: SystemTime,
		n_participants: u32,
		repo_name: String,
		author_id: i64,
		author_login: String,
		matrix_public_ping_count: u32,
	},
	ReviewRequest {
		created_at: SystemTime,
		repo_name: String,
		reviewer_id: i64,
		reviewer_login: String,
		matrix_public_ping_count: u32,
	},
}

#[derive(Debug, Deserialize)]
pub struct Engineer {
	#[serde(rename = "First Name")]
	first_name: String,
	#[serde(rename = "Last Name")]
	last_name: String,
	pub github: Option<String>,
	#[serde(rename = "Riot ID")]
	riot_id: Option<String>,
}

pub struct MatrixSender<'r> {
	pub core: tokio_core::reactor::Core,
	pub mx: MatrixClient,
	pub room: Room<'r>,
}

pub struct GithubBot {
	client: reqwest::Client,
	auth_key: String,
	organisation: github::Organisation,
}

impl GithubBot {
	const BASE_URL: &'static str = "https://api.github.com";

	/// Creates a new instance of `GithubBot` from a GitHub organisation defined by
	/// `org`, and a GitHub authenication key defined by `auth_key`.
	/// # Errors
	/// If the organisation does not exist or `auth_key` does not have sufficent
	/// permissions.
	pub fn new<A: AsRef<str>, I: Into<String>>(org: A, auth_key: I) -> Result<Self> {
		let auth_key = auth_key.into();
		let client = reqwest::Client::new();

		let organisation = client
			.get(&format!("https://api.github.com/orgs/{}", org.as_ref()))
			.bearer_auth(&auth_key)
			.send()
			.context(error::Http)?
			.json()
			.context(error::Http)?;

		Ok(Self {
			client,
			organisation,
			auth_key,
		})
	}

	/// Returns all of the repositories managed by the organisation.
	pub fn repositories(&self) -> Result<Vec<github::Repository>> {
		self.get_all(&self.organisation.repos_url)
	}

	/// Returns all of the pull requests in a single repository.
	pub fn pull_requests(&self, repo: &github::Repository) -> Result<Vec<github::PullRequest>> {
		self.get_all(repo.pulls_url.replace("{/number}", ""))
	}

	/// Returns all reviews associated with a pull request.
	pub fn reviews(&self, pull_request: &github::PullRequest) -> Result<Vec<github::Review>> {
		self.get_all(format!("{}/reviews", pull_request.html_url))
	}
        
	/// Returns all reviews associated with a pull request.
	pub fn issue(&self, pull_request: &github::PullRequest) -> Result<Option<github::Issue>> {
		self.get(&pull_request.links.issue_link.href)
	}

	/// Returns all reviews associated with a pull request.
	pub fn statuses(&self, pull_request: &github::PullRequest) -> Result<Vec<github::Status>> {
		self.get(&pull_request.links.statuses_link.href)
	}

	/// Creates a comment in the r
	pub fn add_comment<A, B>(&self, repo: A, pr: i64, comment: B) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		log::info!("Adding comment");
		let repo = repo.as_ref();
		let comment = comment.as_ref();
		let url = format!(
			"{base}/repos/{org}/{repo}/issues/{pr}/comments",
			base = Self::BASE_URL,
			org = self.organisation.login,
			repo = repo,
			pr = pr
		);
		log::info!("POST {}", url);

		self.client
			.post(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({ "body": comment }))
			.send()
			.context(error::Http)
			.and_then(map_response_status)
			.map(|_| ())
	}

	pub fn assign_author<A, B>(&self, repo: A, pr: i64, author: B) -> Result<()>
	where
		A: AsRef<str>,
		B: AsRef<str>,
	{
		let repo = repo.as_ref();
		let author = author.as_ref();
		let base = &self.organisation.repos_url;
		let url = format!(
			"{base}/{repo}/issues/{pr}/assignees",
			base = base,
			repo = repo,
			pr = pr
		);

		self.client
			.post(&url)
			.bearer_auth(&self.auth_key)
			.json(&serde_json::json!({ "assignees": [author] }))
			.send()
			.context(error::Http)
			.map(|_| ())
	}

	/// Get a single entry from a resource in GitHub.
	fn get<'b, I, T>(&self, url: I) -> Result<T>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
	{
		let mut response = self
			.client
			.get(&*(url.into()))
			.bearer_auth(&self.auth_key)
			.send()
			.context(error::Http)?;

		response.json::<T>().context(error::Http)
	}

	// Originally adapted from:
	// https://github.com/XAMPPRocky/gh-auditor/blob/ca67641c0a29d64fc5c6b4244b45ae601604f3c1/src/lib.rs#L232-L267
	/// Gets a all entries across all pages from a resource in GitHub.
	fn get_all<'b, I, T>(&self, url: I) -> Result<Vec<T>>
	where
		I: Into<Cow<'b, str>>,
		T: serde::de::DeserializeOwned,
	{
		let mut entities = Vec::new();
		let mut next = Some(url.into());

		while let Some(url) = next {
			let mut response = self
				.client
				.get(&*url)
				.bearer_auth(&self.auth_key)
				.send()
				.context(error::Http)?;

			next = response
				.headers()
				.decode::<hyperx::header::Link>()
				.ok()
				.and_then(|v| {
					v.values()
						.iter()
						.find(|link| {
							link.rel()
								.map(|rel| rel.contains(&hyperx::header::RelationType::Next))
								.unwrap_or(false)
						})
						.map(|l| l.link())
						.map(str::to_owned)
						.map(Cow::Owned)
				});

			let mut body = response.json::<Vec<T>>().context(error::Http)?;
			entities.append(&mut body);
		}

		Ok(entities)
	}
}

pub fn update(db: &DB, github_bot: &GithubBot) -> Result<()> {
	let mut live_ids = vec![];
	for repo in github_bot.repositories()? {
		let prs = github_bot.pull_requests(&repo)?;
		for pr in prs {
			let id_bytes = pr.number.to_be_bytes();
			live_ids.push(id_bytes.clone());
			let np = pr.requested_reviewers.len() as u32;

			if let Some(bytes) = db.get_pinned(&id_bytes).context(error::Db)? {
				let entry = serde_json::from_slice(&bytes).context(error::Json)?;
				let (
					author_id,
					author_login,
					created_at,
					matrix_public_ping_count,
					n_participants,
					repo_name,
				) = match entry {
					DbEntry::PullRequest {
						author_id,
						author_login,
						created_at,
						matrix_public_ping_count,
						n_participants,
						repo_name,
					} => (
						author_id,
						author_login,
						created_at,
						matrix_public_ping_count,
						n_participants,
						repo_name,
					),
					_ => unreachable!(),
				};

				if np != n_participants {
					let entry = DbEntry::PullRequest {
						author_id,
						author_login,
						created_at,
						matrix_public_ping_count,
						n_participants: np,
						repo_name,
					};
					db.put(
						id_bytes,
						serde_json::to_string(&entry)
							.expect("serialize PullRequestEntry")
							.as_bytes(),
					)
					.unwrap();
				}
			} else {
				let entry = DbEntry::PullRequest {
					created_at: SystemTime::now(),
					n_participants: np,
					repo_name: repo.name.clone(),
					author_id: pr.user.id,
					author_login: pr.user.login.clone(),
					matrix_public_ping_count: 0,
				};
				db.put(
					id_bytes,
					serde_json::to_string(&entry)
						.expect("serialize PullRequestEntry")
						.as_bytes(),
				)
				.unwrap();
			}

			for reviewer in pr.requested_reviewers {
				let reviewer_id_bytes = reviewer.id.to_be_bytes();
				live_ids.push(reviewer_id_bytes.clone());
				if db.get_pinned(&reviewer_id_bytes).is_ok() {
					let entry = DbEntry::ReviewRequest {
						created_at: SystemTime::now(),
						reviewer_id: reviewer.id,
						repo_name: repo.name.clone(),
						reviewer_login: reviewer.login.clone(),
						matrix_public_ping_count: 0,
					};
					db.put(
						reviewer_id_bytes,
						serde_json::to_string(&entry)
							.expect("serialize ReviewRequestEntry")
							.as_bytes(),
					)
					.unwrap();
				}
			}
		}
	}

	// delete dead entries
	for (key, _) in db.iterator(IteratorMode::Start) {
		if !live_ids.iter().any(|id| id == &*key) {
			db.delete(&key).unwrap();
		}
	}

	Ok(())
}

pub fn act(
	db: &DB,
	github_bot: &GithubBot,
	engineers: &HashMap<String, Engineer>,
	matrix_sender: &mut MatrixSender,
) -> Result<()> {
	for (key, value) in db.iterator(IteratorMode::Start) {
		let pr_id = BigEndian::read_i64(&*key);
		let entry: DbEntry =
			serde_json::from_str(&String::from_utf8_lossy(&value)).context(error::Json)?;
		log::info!("Checking Entry: {:?}", entry);
		match entry {
			DbEntry::PullRequest {
				author_id,
				ref author_login,
				created_at,
				matrix_public_ping_count,
				n_participants,
				ref repo_name,
			} => {
				if let Some(engineer) = engineers.get(author_login) {
					if n_participants < 3 {
						log::info!("Notifying PR author.");
						let since = SystemTime::now()
							.duration_since(created_at)
							.expect("should have been ceated in the past")
							.as_secs() / 3600;
						if since < 24 {
							github_bot.add_comment(
								repo_name,
								pr_id,
								format!(
									"@{}, please assign at least two reviewers.",
									&author_login
								),
							)?;

							github_bot.assign_author(repo_name, pr_id, author_login)?;
						} else if since < 48 {
							// dm author in matrix
							if let Some(riot_id) = &engineer.riot_id {
								let roomopts = RoomCreationOptions {
									visibility: None, // default private
									room_alias_name: None,
									name: None,
									topic: None,
									invite: vec![format!("@{}", riot_id)],
									creation_content: HashMap::new(),
									preset: None,
									is_direct: true,
								};
								let room = matrix_sender
									.core
									.run(NewRoom::create(&mut matrix_sender.mx, roomopts))
									.expect("room for direct chat");
								let mut rc = room.cli(&mut matrix_sender.mx);
								matrix_sender
									.core
									.run(
										rc.send_simple(format!(
											"@{}, please assign at least two reviewers.",
											&riot_id,
										))
										.map(|_| ())
										.map_err(|_| ()),
									)
									.unwrap();
							}
						} else if (since - 48) / 24 > matrix_public_ping_count.into() {
							// ping author in matrix substrate channel
							if let Some(riot_id) = &engineer.riot_id {
								let mut rc = matrix_sender.room.cli(&mut matrix_sender.mx);
								matrix_sender
									.core
									.run(
										rc.send_simple(format!(
											"@{}, please assign at least two reviewers.",
											&riot_id
										))
										.map(|_| ())
										.map_err(|_| ()),
									)
									.unwrap();
							}

							// update ping count
							db.delete(&key).unwrap();
							let entry = DbEntry::PullRequest {
								author_id,
								author_login: author_login.clone(),
								created_at,
								matrix_public_ping_count: ((since - 48) / 24) as u32,
								n_participants,
								repo_name: repo_name.clone(),
							};
							db.put(
								key,
								serde_json::to_string(&entry)
									.expect("serialize PullRequestEntry")
									.as_bytes(),
							)
							.unwrap();
						}
					}
				}
			}

			DbEntry::ReviewRequest {
				created_at,
				matrix_public_ping_count,
				repo_name,
				reviewer_id,
				reviewer_login,
			} => {
				if let Some(engineer) = engineers.get(&reviewer_login) {
					let since = SystemTime::now()
						.duration_since(created_at)
						.expect("should have been ceated in the past")
						.as_secs() / 3600;
					if since < 24 {
					} else if since < 48 {
						github_bot.add_comment(
							repo_name,
							pr_id,
							format!("@{}, please review.", &reviewer_login),
						)?;
					} else if since < 72 {
						if let Some(riot_id) = &engineer.riot_id {
							// dm reviewer in matrix
							let roomopts = RoomCreationOptions {
								visibility: None, // default private,
								room_alias_name: None,
								name: None,
								topic: None,
								invite: vec![format!("@{}", riot_id)],
								creation_content: HashMap::new(),
								preset: None,
								is_direct: true,
							};
							let room = matrix_sender
								.core
								.run(NewRoom::create(&mut matrix_sender.mx, roomopts))
								.expect("room for direct chat");
							let mut rc = room.cli(&mut matrix_sender.mx);
							matrix_sender
								.core
								.run(
									rc.send_simple(format!(
										"@{}, please assign at least two reviewers.",
										&riot_id,
									))
									.map(|_| ())
									.map_err(|_| ()),
								)
								.unwrap();
						}
					} else if (since - 72) / 24 > matrix_public_ping_count.into() {
						// ping reviewer in matrix substrate channel
						if let Some(riot_id) = &engineer.riot_id {
							let mut rc = matrix_sender.room.cli(&mut matrix_sender.mx);
							matrix_sender
								.core
								.run(
									rc.send_simple(format!(
										"@{}, please assign at least two reviewers.",
										&riot_id,
									))
									.map(|_| ())
									.map_err(|_| ()),
								)
								.unwrap();
						}

						// update ping count
						db.delete(&key).unwrap();
						let entry = DbEntry::ReviewRequest {
							created_at,
							reviewer_id,
							repo_name,
							reviewer_login,
							matrix_public_ping_count: ((since - 72) / 24) as u32,
						};
						db.put(
							key,
							serde_json::to_string(&entry)
								.expect("serialize PullRequestEntry")
								.as_bytes(),
						)
						.unwrap();
					}
				}
			}
		}
	}
	Ok(())
}
