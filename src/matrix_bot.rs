use std::collections::HashMap;

use crate::matrix;
use crate::Result;

pub struct MatrixBot {
	homeserver: String,
	access_token: String,
	default_channel_id: String,
}

impl MatrixBot {
	pub fn new(
		homeserver: &str,
		username: &str,
		password: &str,
		default_channel_id: &str,
	) -> Result<Self> {
		matrix::login(homeserver, username, password).map(
			|matrix::LoginResponse { access_token }| Self {
				homeserver: homeserver.to_owned(),
				access_token: access_token,
				default_channel_id: default_channel_id.to_owned(),
			},
		)
	}

	pub fn message_mapped_or_default(
		&self,
		github_to_matrix: &HashMap<String, String>,
		github_login: &str,
		msg: &str,
	) -> Result<()> {
		if let Some(matrix_id) = github_to_matrix
			.get(github_login)
			.and_then(|matrix_id| matrix::parse_id(matrix_id))
		{
			self.send_private_message(&matrix_id, msg)
		} else {
			log::warn!(
            "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
            github_login
        );
			self.send_to_default(msg)
		}
	}

	pub fn send_private_message(&self, user_id: &str, msg: &str) -> Result<()> {
		matrix::create_room(&self.homeserver, &self.access_token).and_then(
			|matrix::CreateRoomResponse { room_id }| {
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
		)
	}

	pub fn send_to_room(&self, room_id: &str, msg: &str) -> Result<()> {
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
		if let Some(ref room_id) = room_id {
			self.send_to_room(&room_id, msg)
		} else {
			self.send_to_room(&self.default_channel_id, msg)
		}
	}

	pub fn send_to_default(&self, msg: &str) -> Result<()> {
		self.send_to_room(&self.default_channel_id, msg)
	}
}
