use std::borrow::Cow;
use std::collections::HashMap;
use std::time::SystemTime;

use byteorder::{BigEndian, ByteOrder};
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
        author_id: i64,
        author_login: String,
        created_at: SystemTime,
        last_ping: Option<SystemTime>,
        n_participants: u32,
        repo_name: String,
    },
    ReviewRequest {
        created_at: SystemTime,
        last_ping: Option<SystemTime>,
        repo_name: String,
        reviewer_id: i64,
        reviewer_login: String,
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

impl<'r> MatrixSender<'r> {
    fn direct_message<A: AsRef<str>>(&mut self, invitees: &[&str], message: A) -> Result<()> {
        let message = message.as_ref();
        let options = RoomCreationOptions {
            visibility: None, // default private
            room_alias_name: None,
            name: None,
            topic: None,
            invite: invitees.iter().map(|s| format!("@{}", s)).collect(),
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

pub struct Bot {
    client: reqwest::Client,
    auth_key: String,
    organisation: github::Organisation,
}

impl Bot {
    const BASE_URL: &'static str = "https://api.github.com";

    /// Creates a new instance of `Bot` from a GitHub organisation defined by
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
}

pub fn update(db: &DB, bot: &Bot) -> Result<()> {
    let mut live_ids = vec![];
    for repo in bot.repositories()? {
        let prs = bot.pull_requests(&repo)?;
        for pr in prs {
            let id_bytes = pr.number.to_be_bytes();
            live_ids.push(id_bytes.clone());
            let np = pr.requested_reviewers.len() as u32;

            if let Some(bytes) = db.get_pinned(&id_bytes).context(error::Db)? {
                let entry = serde_json::from_slice(&bytes).context(error::Json)?;
                let (author_id, author_login, created_at, last_ping, n_participants, repo_name) =
                    match entry {
                        DbEntry::PullRequest {
                            author_id,
                            author_login,
                            created_at,
                            last_ping,
                            n_participants,
                            repo_name,
                        } => (
                            author_id,
                            author_login,
                            created_at,
                            last_ping,
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
                        last_ping,
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
                    last_ping: None,
                    n_participants: np,
                    repo_name: repo.name.clone(),
                    author_id: pr.user.id,
                    author_login: pr.user.login.clone(),
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
                        last_ping: None,
                        reviewer_id: reviewer.id,
                        repo_name: repo.name.clone(),
                        reviewer_login: reviewer.login.clone(),
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
    bot: &Bot,
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
                created_at,
                last_ping,
                n_participants,
                ref author_login,
                ref repo_name,
            } => {
                let engineer = match engineers.get(author_login) {
                    Some(eng) => eng,
                    _ => continue,
                };

                if n_participants >= 2 {
                    continue;
                }

                let since = SystemTime::now()
                    .duration_since(last_ping.unwrap_or(created_at))
                    .expect("should have been ceated in the past")
                    .as_secs()
                    / 3600;

                log::info!("{} hours since last check", since);

                let last_ping = match since {
                    0..=24 if last_ping.is_none() => {
                        bot.add_comment(
                            repo_name,
                            pr_id,
                            format!(
                                "Hello @{}, please ensure to assign at \
                                 least two reviewers, Otherwise this PR will \
                                 be closed in the near future.",
                                &author_login
                            ),
                        )?;

                        bot.assign_author(repo_name, pr_id, author_login)?;

                        Some(SystemTime::now())
                    }

                    25..=48 if engineer.riot_id.is_some() => {
                        let riot_id = engineer.riot_id.as_ref().unwrap();
                        // dm author in matrix
                        let message = format!(
                            "@{author}, please assign at least two reviewers \
                             to {link}. This PR will close after 72 hours if \
                             reviewers haven't been assigned.",
                            author = riot_id,
                            link = "hello"
                        );

                        matrix_sender.direct_message(&[&riot_id], &message)?;

                        Some(SystemTime::now())
                    }

                    49..=72 if engineer.riot_id.is_some() => {
                        let riot_id = engineer.riot_id.as_ref().unwrap();
                        // dm author in matrix
                        let message = format!(
                            "@{author}, please assign at least two \
                             reviewers to {link}. This PR will close after \
                             48 hours if reviewers haven't been assigned.",
                            author = riot_id,
                            link = "hello"
                        );

                        matrix_sender.room_message("substrate", &message)?;

                        Some(SystemTime::now())
                    }

                    _ => last_ping,
                };

                // update ping count
                db.delete(&key).unwrap();
                let entry = DbEntry::PullRequest {
                    author_id,
                    author_login: author_login.clone(),
                    created_at,
                    last_ping,
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

            DbEntry::ReviewRequest {
                created_at,
                last_ping,
                repo_name,
                reviewer_id,
                reviewer_login,
            } => {
                let engineer = match engineers.get(&reviewer_login) {
                    Some(eng) => eng,
                    _ => continue,
                };

                let riot_id = match engineer.riot_id {
                    Some(ref id) => id,
                    _ => continue,
                };

                let since = SystemTime::now()
                    .duration_since(last_ping.unwrap_or(created_at))
                    .expect("should have been ceated in the past")
                    .as_secs()
                    / 3600;

                let last_ping = match since {
                    24..=48 if last_ping.is_none() => {
                        bot.add_comment(
                            &repo_name,
                            pr_id,
                            format!("@{}, please review.", &reviewer_login),
                        )?;
                        Some(SystemTime::now())
                    }
                    24..=72 => {
                        // dm reviewer in matrix
                        matrix_sender.direct_message(
                            &[&riot_id],
                            format!("@{}, please review.", &riot_id),
                        )?;
                        last_ping
                    }
                    73..=128 => {
                        matrix_sender.room_message(
                            "parity-bots",
                            format!("@{}, please review.", &riot_id),
                        )?;

                        Some(SystemTime::now())
                    }
                    _ => last_ping,
                };

                // update entry
                db.delete(&key).unwrap();
                let entry = DbEntry::ReviewRequest {
                    created_at,
                    reviewer_id,
                    last_ping,
                    repo_name,
                    reviewer_login,
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
    Ok(())
}
