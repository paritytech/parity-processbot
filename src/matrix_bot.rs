use parking_lot::RwLock;
use rocksdb::DB;
use snafu::ResultExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{error, matrix, Result};

#[derive(Debug)]
pub struct MatrixBot {
	homeserver: String,
	access_token: String,
	default_channel_id: String,
	silent: bool,
}

impl MatrixBot {
	pub fn new_with_token(
		homeserver: &str,
		access_token: &str,
		default_channel_id: &str,
		silent: bool,
	) -> Result<Self> {
		matrix::sync(homeserver, access_token)
			.map(|_| Self {
				homeserver: homeserver.to_owned(),
				access_token: access_token.to_owned(),
				default_channel_id: default_channel_id.to_owned(),
				silent: silent,
			})
			.map_err(|e| {
				log::error!("Error syncing with Matrix: {}", e);
				e
			})
	}

	pub fn new_with_credentials(
		homeserver: &str,
		username: &str,
		password: &str,
		default_channel_id: &str,
		silent: bool,
	) -> Result<Self> {
		matrix::login(homeserver, username, password).map(
			|matrix::LoginResponse { access_token }| Self {
				homeserver: homeserver.to_owned(),
				access_token: access_token,
				default_channel_id: default_channel_id.to_owned(),
				silent: silent,
			},
		)
	}

	pub fn new_placeholder_for_testing() -> Self {
		Self {
			homeserver: "".to_string(),
			access_token: "".to_string(),
			default_channel_id: "".to_string(),
			silent: true,
		}
	}

	pub fn message_mapped(
		&self,
		db: &Arc<RwLock<DB>>,
		github_to_matrix: &HashMap<String, String>,
		github_login: &str,
		msg: &str,
	) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		if let Some(matrix_id) = github_to_matrix
			.get(github_login)
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
		{
			self.send_private_message(db, &matrix_id, msg)
		} else {
			log::error!(
				"Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
				github_login
			);
			Ok(())
		}
	}

	pub fn message_mapped_or_default(
		&self,
		db: &Arc<RwLock<DB>>,
		github_to_matrix: &HashMap<String, String>,
		github_login: &str,
		msg: &str,
	) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		if let Some(matrix_id) = github_to_matrix
			.get(github_login)
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
		{
			self.send_private_message(db, &matrix_id, msg)
		} else {
			log::error!(
				"Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
				github_login
			);
			self.send_to_default(msg)
		}
	}

	pub fn send_private_message(
		&self,
		db: &Arc<RwLock<DB>>,
		user_id: &str,
		msg: &str,
	) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		let db = db.write();
		if let Some(room_id) = db
			.get_pinned(user_id)
			.context(error::Db)?
			.and_then(|v| String::from_utf8(v.to_vec()).ok())
		{
			matrix::send_message(
				&self.homeserver,
				&self.access_token,
				&room_id,
				msg,
			)?
		} else {
			matrix::create_room(&self.homeserver, &self.access_token).and_then(
				|matrix::CreateRoomResponse { room_id }| {
					db.put(user_id, room_id.as_bytes()).context(error::Db)?;
					matrix::invite(
						&self.homeserver,
						&self.access_token,
						&room_id,
						user_id,
					)?;
					matrix::send_message(
						&self.homeserver,
						&self.access_token,
						&room_id,
						msg,
					)
				},
			)?
		}
		Ok(())
	}

	pub fn send_to_room(&self, room_id: &str, msg: &str) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		matrix::send_message(
			&self.homeserver,
			&self.access_token,
			&room_id,
			msg,
		)
	}

	pub fn send_to_room_or_default(
		&self,
		room_id: Option<&String>,
		msg: &str,
	) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		if let Some(ref room_id) = room_id {
			self.send_to_room(&room_id, msg)
		} else {
			self.send_to_room(&self.default_channel_id, msg)
		}
	}

	pub fn send_to_default(&self, msg: &str) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		self.send_to_room(&self.default_channel_id, msg)
	}

	pub fn send_html_to_default(&self, msg: &str) -> Result<()> {
		if self.silent {
			return Ok(());
		};
		matrix::send_html_message(
			&self.homeserver,
			&self.access_token,
			&self.default_channel_id,
			msg,
		)
	}
}
