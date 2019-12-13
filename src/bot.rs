use crate::gm::room::{NewRoom, RoomExt};
use futures::future::Future;
use gm::types::replies::RoomCreationOptions;
use gm::types::room::Room;
use gm::MatrixClient;
use graphql_client::*;
use log::*;
use rocksdb::{IteratorMode, DB};
use serde::de::DeserializeOwned;
use serde::*;
use std::collections::HashMap;
use std::time::SystemTime;

use organization::OrganizationOrganizationRepositoriesNodesPullRequestsNodesAuthorOn::User as AuthorUser;
use organization::OrganizationOrganizationRepositoriesNodesPullRequestsNodesReviewRequestsNodesRequestedReviewer::User as ReviewerUser;

#[derive(GraphQLQuery)]
#[graphql(
	schema_path = "src/schema.public.graphql",
	query_path = "src/query_1.graphql",
	response_derives = "Debug,Serialize"
)]
struct Organization;

#[derive(GraphQLQuery)]
#[graphql(
	schema_path = "src/schema.public.graphql",
	query_path = "src/query_1.graphql",
	response_derives = "Debug"
)]
struct AddComment;

#[derive(GraphQLQuery)]
#[graphql(
	schema_path = "src/schema.public.graphql",
	query_path = "src/query_1.graphql",
	response_derives = "Debug"
)]
struct AssignAuthor;

fn send_query<Q: Serialize + Sized, R: DeserializeOwned>(
	client: &reqwest::Client,
	github_token: &str,
	query: Q,
) -> Result<R, failure::Error> {
	let mut res = client
		.post("https://api.github.com/graphql")
		.bearer_auth(github_token.clone())
		.json(&query)
		.send()?;

	let response_body: Response<R> = res.json()?;

	if let Some(errors) = response_body.errors {
		for error in &errors {
			debug!("{:?}", error);
		}
	}

	Ok(response_body.data.expect("response data"))
}

fn mutate_add_comment(
	client: &reqwest::Client,
	github_token: &str,
	pr_id: String,
	body: String,
) -> Result<add_comment::ResponseData, failure::Error> {
	let q = AddComment::build_query(add_comment::Variables {
		input: add_comment::AddCommentInput {
			body: body,
			subject_id: pr_id,
			client_mutation_id: None,
		},
	});

	send_query(client, github_token, q)
}

fn mutate_assign_author(
	client: &reqwest::Client,
	github_token: &str,
	pr_id: String,
	author_id: String,
) -> Result<assign_author::ResponseData, failure::Error> {
	let q = AssignAuthor::build_query(assign_author::Variables {
		input: assign_author::AddAssigneesToAssignableInput {
			assignable_id: pr_id,
			assignee_ids: vec![author_id],
			client_mutation_id: None,
		},
	});

	send_query(client, github_token, q)
}

#[derive(Serialize, Deserialize, Debug)]
enum DbEntry {
	PullRequest {
		created_at: SystemTime,
		n_participants: u32,
		author_id: String,
		author_login: String,
		matrix_public_ping_count: u32,
	},
	ReviewRequest {
		created_at: SystemTime,
		reviewer_id: String,
		reviewer_login: String,
		matrix_public_ping_count: u32,
	},
}

#[derive(Debug, Deserialize)]
pub struct Engineer {
	first_name: Option<String>,
	last_name: Option<String>,
	pub github: Option<String>,
	riot_id: Option<String>,
}

pub struct MatrixSender<'r> {
	pub core: tokio_core::reactor::Core,
	pub mx: MatrixClient,
	pub room: Room<'r>,
}

pub fn update(db: &DB, github_token: &str, org: &str) -> Result<(), failure::Error> {
	let q = Organization::build_query(organization::Variables {
		login: org.to_string(),
	});

	let client = reqwest::Client::new();
	let org_response: organization::ResponseData =
		send_query(&client, github_token, q).expect("organization response");

	let repos = org_response
		.organization
		.expect("repository")
		.repositories
		.nodes
		.expect("nodes");

	let mut live_ids = vec![];
	for repo in repos {
		if let Some(prs) = repo.and_then(|r| r.pull_requests.nodes) {
			for pr in prs {
				if let Some(pr) = pr {
					live_ids.push(pr.id.clone().into_bytes());
					let np = pr.participants.total_count as u32;
					if let Ok(Some(value)) = db.get_pinned(pr.id.as_bytes()) {
						if let DbEntry::PullRequest {
							created_at,
							n_participants,
							author_id,
							author_login,
							matrix_public_ping_count,
						} = serde_json::from_str(
							String::from_utf8(value.to_vec()).unwrap().as_str(),
						)
						.expect("deserialize entry")
						{
							if np != n_participants {
								let entry = DbEntry::PullRequest {
									created_at: created_at,
									n_participants: np,
									author_id: author_id.clone(),
									author_login: author_login.clone(),
									matrix_public_ping_count: matrix_public_ping_count,
								};
								db.put(
									pr.id.as_bytes(),
									serde_json::to_string(&entry)
										.expect("serialize PullRequestEntry")
										.as_bytes(),
								)
								.unwrap();
							}
						} else {
                                                        panic!("pr id is somehow the key for a review request");
						}
					} else if let AuthorUser(author) = pr
						.author
						.as_ref()
						.map(|auth| &auth.on)
						.expect("pull request should have an author")
					{
						let entry = DbEntry::PullRequest {
							created_at: SystemTime::now(),
							n_participants: np,
							author_id: author.id.clone(),
							author_login: author.login.clone(),
							matrix_public_ping_count: 0,
						};
						db.put(
							pr.id.as_bytes(),
							serde_json::to_string(&entry)
								.expect("serialize PullRequestEntry")
								.as_bytes(),
						)
						.unwrap();
					} else {
						// PR author was not a user
					}

					if let Some(review_requests) =
						pr.review_requests.as_ref().and_then(|r| r.nodes.as_ref())
					{
						for rev in review_requests {
							if let Some(rev) = rev {
								live_ids.push(rev.id.clone().into_bytes());
								if db.get_pinned(rev.id.as_bytes()).is_ok() {
								} else if let ReviewerUser(reviewer) = rev
									.requested_reviewer
									.as_ref()
									.map(|auth| auth)
									.expect("review request should have a reviewer")
								{
									let entry = DbEntry::ReviewRequest {
										created_at: SystemTime::now(),
										reviewer_id: reviewer.id.clone(),
										reviewer_login: reviewer.login.clone(),
										matrix_public_ping_count: 0,
									};
									db.put(
										rev.id.as_bytes(),
										serde_json::to_string(&entry)
											.expect("serialize ReviewRequestEntry")
											.as_bytes(),
									)
									.unwrap();
								} else {
									// reviewer was not a user
								}
							}
						}
					}
				}
			}
		}
	}
	// delete dead entries
	for (key, _) in db.iterator(IteratorMode::Start) {
		if !live_ids.contains(&key.to_vec().as_ref()) {
			db.delete(&key).unwrap();
		}
	}

	Ok(())
}

pub fn act(
	db: &DB,
	github_token: &str,
	engineers: &HashMap<String, Engineer>,
	matrix_sender: &mut MatrixSender,
) -> Result<(), failure::Error> {
	let client = reqwest::Client::new();
	for (key, value) in db.iterator(IteratorMode::Start) {
		let pr_id = String::from_utf8(key.to_vec()).unwrap();
		let entry: DbEntry =
			serde_json::from_str(String::from_utf8(value.to_vec()).unwrap().as_str())
				.expect("deserialize entry");
		match entry {
			DbEntry::PullRequest {
				created_at,
				n_participants,
				author_id,
				author_login,
				matrix_public_ping_count,
			} => {
				if let Some(engineer) = engineers.get(&author_login) {
					if n_participants < 3 {
						let since = SystemTime::now()
							.duration_since(created_at)
							.expect("should have been ceated in the past")
							.as_secs() / 3600;
						if since < 24 {
							// ping author in github
							let _ = mutate_add_comment(
								&client,
								github_token,
								pr_id.clone(),
								format!(
									"@{}, please assign at least two reviewers.",
									&author_login
								),
							);
							let _ = mutate_assign_author(
								&client,
								github_token,
								pr_id.clone(),
								author_id.clone(),
							);
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
								matrix_sender.core.run(
									rc.send_simple(format!(
										"@{}, please assign at least two reviewers.",
										&riot_id,
									))
									.map(|_| ())
									.map_err(|_| ()),
								).unwrap();
							}
						} else if (since - 48) / 24 > matrix_public_ping_count.into() {
							// ping author in matrix substrate channel
							if let Some(riot_id) = &engineer.riot_id {
								let mut rc = matrix_sender.room.cli(&mut matrix_sender.mx);
								matrix_sender.core.run(
									rc.send_simple(format!(
										"@{}, please assign at least two reviewers.",
										&riot_id
									))
									.map(|_| ())
									.map_err(|_| ()),
								).unwrap();
							}

							// update ping count
							db.delete(&key).unwrap();
							let entry = DbEntry::PullRequest {
								created_at: created_at,
								n_participants: n_participants,
								author_id: author_id,
								author_login: author_login,
								matrix_public_ping_count: ((since - 48) / 24) as u32,
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
				reviewer_id,
				reviewer_login,
				matrix_public_ping_count,
			} => {
				if let Some(engineer) = engineers.get(&reviewer_login) {
					let since = SystemTime::now()
						.duration_since(created_at)
						.expect("should have been ceated in the past")
						.as_secs() / 3600;
					if since < 24 {
					} else if since < 48 {
						// ping reviewer in github
						let _ = mutate_add_comment(
							&client,
							github_token,
							pr_id.clone(),
							format!("@{}, please review.", &reviewer_login),
						);
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
							matrix_sender.core.run(
								rc.send_simple(format!(
									"@{}, please assign at least two reviewers.",
									&riot_id,
								))
								.map(|_| ())
								.map_err(|_| ()),
							).unwrap();
						}
					} else if (since - 72) / 24 > matrix_public_ping_count.into() {
						// ping reviewer in matrix substrate channel
						if let Some(riot_id) = &engineer.riot_id {
							let mut rc = matrix_sender.room.cli(&mut matrix_sender.mx);
							matrix_sender.core.run(
								rc.send_simple(format!(
									"@{}, please assign at least two reviewers.",
									&riot_id,
								))
								.map(|_| ())
								.map_err(|_| ()),
							).unwrap();
						}

						// update ping count
						db.delete(&key).unwrap();
						let entry = DbEntry::ReviewRequest {
							created_at: created_at,
							reviewer_id: reviewer_id,
							reviewer_login: reviewer_login,
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
