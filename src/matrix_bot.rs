use crate::matrix;
use crate::Result;

pub struct MatrixBot {
	homeserver: String,
	access_token: String,
}

impl MatrixBot {
	pub fn new(homeserver: &str, username: &str, password: &str) -> Result<Self> {
		matrix::login(homeserver, username, password).map(
			|matrix::LoginResponse { access_token }| Self {
				homeserver: homeserver.to_owned(),
				access_token: access_token,
			},
		)
	}

	pub fn send_private_message(&self, user_id: &str, msg: &str) -> Result<()> {
		matrix::create_room(&self.homeserver, &self.access_token).and_then(
			|matrix::CreateRoomResponse { room_id }| {
				matrix::invite(&self.homeserver, &self.access_token, &room_id, user_id)?;
				matrix::send_message(&self.homeserver, &self.access_token, &room_id, msg)
			},
		)
	}

	pub fn send_public_message(&self, room_id: &str, msg: &str) -> Result<()> {
		matrix::send_message(&self.homeserver, &self.access_token, &room_id, msg)
	}
}
