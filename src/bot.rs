use std::borrow::Cow;
use std::collections::HashMap;
use std::time::SystemTime;

use gm::room::{NewRoom, RoomExt};
use gm::types::replies::RoomCreationOptions;
use gm::types::room::Room;
use gm::MatrixClient;
use hyperx::header::TypedHeaders;
use rocksdb::{IteratorMode, DB};
use serde::*;
use snafu::{OptionExt, ResultExt};

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

#[derive(Debug, Deserialize, Serialize)]
pub struct PullRequestData {
    created_at: SystemTime,
    engineer: Engineer,
    gh_metadata: GitHubMetadata,
    last_ping: Option<SystemTime>,
    n_participants: u32,
    project_room: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum DbEntry {
    PullRequest(PullRequestData),
    ReviewRequest {
        created_at: SystemTime,
        gh_metadata: GitHubMetadata,
        last_ping: Option<SystemTime>,
        project_room: String,
        reviewer: Engineer,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GitHubMetadata {
    repo_name: String,
    pr_number: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

impl<'r> MatrixSender<'r> {
    fn direct_message<A: AsRef<str>>(&mut self, invitees: &[&str], message: A) -> Result<()> {
        log::info!("Sending DM.");
        let message = message.as_ref();
        let options = RoomCreationOptions {
            visibility: None, // default private
            room_alias_name: None,
            name: None,
            topic: None,
            invite: invitees.iter().map(|s| String::from(*s)).collect(),
            creation_content: HashMap::new(),
            preset: None,
            is_direct: true,
        };

        let room = self
            .core
            .run(NewRoom::create(&mut self.mx, options))
            .unwrap();

        let mut room = room.cli(&mut self.mx);

        self.core.run(room.send_simple(message)).unwrap();

        Ok(())
    }

    fn room_message<A: AsRef<str>>(&mut self, room: &str, message: A) -> Result<()> {
        let message = message.as_ref();
        let room = self.core.run(NewRoom::join(&mut self.mx, room)).unwrap();
        let mut room = room.cli(&mut self.mx);
        self.core.run(room.send_simple(message)).unwrap();

        Ok(())
    }
}

#[derive(Default)]
pub struct BotBuilder<'a> {
    org: String,
    client: Option<reqwest::Client>,
    matrix: Option<MatrixSender<'a>>,
    db: Option<DB>,
    auth_key: Option<String>,
    engineers: Option<HashMap<String, Engineer>>,
}

impl<'a> BotBuilder<'a> {
    pub fn new(org: String) -> Self {
        Self {
            org,
            ..Self::default()
        }
    }

    pub fn client(mut self, client: reqwest::Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn matrix(mut self, matrix: MatrixSender<'a>) -> Self {
        self.matrix = Some(matrix);
        self
    }

    pub fn db(mut self, db: DB) -> Self {
        self.db = Some(db);
        self
    }

    pub fn auth_key(mut self, auth_key: String) -> Self {
        self.auth_key = Some(auth_key);
        self
    }

    pub fn engineers(mut self, engineers: HashMap<String, Engineer>) -> Self {
        self.engineers = Some(engineers);
        self
    }

    /// Creates a new instance of `Bot` from a GitHub organisation defined by
    /// `org`, and a GitHub authenication key defined by `auth_key`.
    /// # Errors
    /// If the organisation does not exist or `auth_key` does not have sufficent
    /// permissions.
    pub fn finish(self) -> Result<Bot<'a>> {
        let client = self.client.unwrap_or_else(reqwest::Client::new);
        let matrix = self.matrix.context(error::BotCreation {
            msg: "Matrix client missing.",
        })?;
        let db = self.db.context(error::BotCreation {
            msg: "Rocksdb client missing.",
        })?;
        let auth_key = self.auth_key.context(error::BotCreation {
            msg: "GitHub auth key missing.",
        })?;
        let engineers = self.engineers.context(error::BotCreation {
            msg: "Engineers missing.",
        })?;

        let organisation = client
            .get(&format!("https://api.github.com/orgs/{}", self.org))
            .bearer_auth(&auth_key)
            .send()
            .context(error::Http)?
            .json()
            .context(error::Http)?;

        Ok(Bot {
            client,
            matrix,
            db,
            auth_key,
            engineers,
            organisation,
        })
    }
}

pub struct Bot<'a> {
    client: reqwest::Client,
    matrix: MatrixSender<'a>,
    db: DB,
    auth_key: String,
    engineers: HashMap<String, Engineer>,
    organisation: github::Organisation,
}

impl<'a> Bot<'a> {
    const BASE_URL: &'static str = "https://api.github.com";

    /// Returns all of the repositories managed by the organisation.
    pub fn repositories(&self) -> Result<Vec<github::Repository>> {
        self.get_all(&self.organisation.repos_url)
    }

    /// Returns all of the pull requests in a single repository.
    pub fn pull_requests(&self, repo: &github::Repository) -> Result<Vec<github::PullRequest>> {
        self.get_all(repo.pulls_url.replace("{/number}", ""))
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
        let url = format!(
            "{base}/repos/{org}/{repo}/issues/{pr}/assignees",
            base = Self::BASE_URL,
            org = self.organisation.login,
            repo = repo,
            pr = pr
        );

        self.client
            .post(&url)
            .bearer_auth(&self.auth_key)
            .json(&serde_json::json!({ "assignees": [author] }))
            .send()
            .context(error::Http)
            .and_then(map_response_status)
            .map(|_| ())
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

    /// Attempts to ping someon on GitHub, matrix (private), and matrix (public)
    /// depending on how long since the last ping. Returns the time it last
    /// pinged if it did.
    pub fn ping_person<A>(
        &mut self,
        engineer: &Engineer,
        gh_meta: &GitHubMetadata,
        project_room: &str,
        created_at: SystemTime,
        last_ping: Option<SystemTime>,
        message: A,
    ) -> Result<Option<SystemTime>>
    where
        A: AsRef<str>,
    {
        let message = message.as_ref();
        // TODO: Make Github login a requirement.
        let github_login = match engineer.github {
            Some(ref gh) => gh,
            _ => return Ok(last_ping),
        };
        let since = SystemTime::now()
            .duration_since(last_ping.unwrap_or(created_at))
            .expect("should have been ceated in the past")
            .as_secs()
            / 3600;

        log::info!("{} hours since last check", since);

        let last_ping = match since {
            0..=24 if last_ping.is_none() => {
                self.add_comment(
                    &gh_meta.repo_name,
                    gh_meta.pr_number,
                    format!(
                        "Hello @{author}, {message}",
                        author = github_login,
                        message = message,
                    ),
                )?;

                self.assign_author(&gh_meta.repo_name, gh_meta.pr_number, github_login)?;

                Some(SystemTime::now())
            }

            25..=48 if engineer.riot_id.is_some() => {
                let riot_id = engineer.riot_id.as_ref().unwrap();
                // dm author in matrix
                let message = format!("{author}, {message}", author = riot_id, message = message);

                self.matrix.direct_message(&[&riot_id], &message)?;

                Some(SystemTime::now())
            }

            49..=72 if engineer.riot_id.is_some() => {
                let riot_id = engineer.riot_id.as_ref().unwrap();
                // dm author in matrix
                let message = format!("{author}, {message}", author = riot_id, message = message);
                self.matrix.room_message(project_room, &message)?;

                Some(SystemTime::now())
            }

            _ => last_ping,
        };

        Ok(last_ping)
    }

    pub fn update_entry(&self, key: &[u8], entry: DbEntry) -> Result<()> {
        self.db.delete(&key).context(error::Db)?;

        self.db
            .put(
                key,
                serde_json::to_string(&entry)
                    .context(error::Json)?
                    .as_bytes(),
            )
            .context(error::Db)
    }

    pub fn act(&mut self) -> Result<()> {
        for (key, value) in self.db.iterator(IteratorMode::Start) {
            let entry: DbEntry =
                serde_json::from_str(&String::from_utf8_lossy(&value)).context(error::Json)?;
            log::info!("Checking Entry: {:?}", entry);

            match entry {
                DbEntry::PullRequest(mut data) => {
                    if data.n_participants >= 2 {
                        continue;
                    } else {
                        data.last_ping = self.ping_person(
                            &data.engineer,
                            &data.gh_metadata,
                            &data.project_room,
                            data.created_at,
                            data.last_ping,
                            "Please assign at least 2 reviewers.",
                        )?;

                        self.update_entry(&key, DbEntry::PullRequest(data))?;
                    }
                }

                DbEntry::ReviewRequest {
                    created_at,
                    last_ping,
                    project_room,
                    reviewer,
                    gh_metadata,
                } => {
                    let last_ping = self.ping_person(
                        &reviewer,
                        &gh_metadata,
                        &project_room,
                        created_at,
                        last_ping,
                        "Please review",
                    )?;

                    self.update_entry(
                        &key,
                        DbEntry::ReviewRequest {
                            created_at,
                            last_ping,
                            project_room,
                            reviewer,
                            gh_metadata,
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        let mut live_ids = vec![];
        for repo in self.repositories()? {
            let prs = self.pull_requests(&repo)?;
            for pr in prs {
                let engineer = match self.engineers.get(&pr.user.login) {
                    Some(eng) => eng,
                    _ => continue,
                };

                let id_bytes = pr.id.to_be_bytes();
                live_ids.push(id_bytes.clone());
                let np = pr.requested_reviewers.len() as u32;

                if let Some(bytes) = self.db.get_pinned(&id_bytes).context(error::Db)? {
                    let entry = serde_json::from_slice(&bytes).context(error::Json)?;
                    let mut data = match entry {
                        DbEntry::PullRequest(data) => data,
                        _ => unreachable!(),
                    };

                    if np != data.n_participants {
                        data.n_participants = np;
                        self.db
                            .put(
                                id_bytes,
                                serde_json::to_string(&DbEntry::PullRequest(data))
                                    .context(error::Json)?
                                    .as_bytes(),
                            )
                            .context(error::Db)?;
                    }
                } else {
                    let data = PullRequestData {
                        created_at: SystemTime::now(),
                        last_ping: None,
                        n_participants: np,
                        // TODO: Replace clone with reference.
                        engineer: engineer.clone(),
                        // TODO: Fetch project room info.
                        project_room: "parity-bots".to_string(),
                        gh_metadata: GitHubMetadata {
                            repo_name: repo.name.clone(),
                            pr_number: pr.number,
                        },
                    };

                    self.db
                        .put(
                            id_bytes,
                            serde_json::to_string(&DbEntry::PullRequest(data))
                                .context(error::Json)?
                                .as_bytes(),
                        )
                        .context(error::Db)?;
                }

                for reviewer in pr.requested_reviewers {
                    let reviewer_id_bytes = reviewer.id.to_be_bytes();
                    live_ids.push(reviewer_id_bytes.clone());
                    if self.db.get_pinned(&reviewer_id_bytes).is_ok() {
                        let entry = DbEntry::ReviewRequest {
                            created_at: SystemTime::now(),
                            last_ping: None,
                            // TODO: Replace clone with reference.
                            reviewer: (*engineer).clone(),
                            // TODO: Get project room info
                            project_room: "parity-bots".to_string(),
                            gh_metadata: GitHubMetadata {
                                pr_number: pr.number,
                                repo_name: repo.name.clone(),
                            },
                        };

                        self.update_entry(&reviewer_id_bytes, entry)?;
                    }
                }
            }
        }

        // delete dead entries
        for (key, _) in self.db.iterator(IteratorMode::Start) {
            if !live_ids.iter().any(|id| id == &*key) {
                self.db.delete(&key).unwrap();
            }
        }

        Ok(())
    }
}
