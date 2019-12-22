use byteorder::{
	BigEndian,
	ByteOrder,
};
use futures::future::Future;
use hyperx::header::TypedHeaders;
use rocksdb::{
	IteratorMode,
	DB,
};
use serde::*;
use snafu::ResultExt;
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::SystemTime;

use crate::{
	error,
	github,
	github_bot::GithubBot,
	matrix_bot::MatrixBot,
	project,
	pull_request::handle_pull_request,
	Result,
};

pub fn update(
	db: &DB,
	github_bot: &GithubBot,
	matrix_bot: &MatrixBot,
	core_devs: &[github::User],
	github_to_matrix: &HashMap<String, String>,
) -> Result<()> {
	for repo in github_bot.repositories()? {
		let projects = github_bot
			.contents(&repo, "Project.toml")
			.map_err(anyhow::Error::new)
			.and_then(|c| {
				toml::from_str::<toml::value::Table>(&dbg!(c).content).map_err(anyhow::Error::new)
			})
			.map(project::Projects::from)
			.map(|p| p.0);
		let project_info = if let Ok(ref projects) = projects {
			projects.get(&repo.name)
		} else {
			None
		};

		let prs = github_bot.pull_requests(&repo)?;
		for pr in prs {
			handle_pull_request(
				db,
				github_bot,
				matrix_bot,
				core_devs,
				github_to_matrix,
				project_info,
				&pr,
			)?;
		}
	}
	Ok(())
}

/*
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
*/
