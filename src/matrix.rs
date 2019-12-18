use crate::developer::Developer;
use curl::easy::Easy;
use serde::{Deserialize, Serialize};
use std::io::{stdout, Write};

#[derive(Deserialize, Debug)]
pub struct LoginResponse {
	pub access_token: String,
}

#[derive(Deserialize, Debug)]
pub struct CreateRoomResponse {
	pub room_id: String,
}

pub fn login(homeserver: &str, username: &str, password: &str) -> LoginResponse {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url(format!("{}/_matrix/client/r0/login", homeserver).as_ref())
		.unwrap();
	handle
                .post_fields_copy(
                        format!(
                                "{{\"type\":\"m.login.password\", \"identifier\": {{ \"type\": \"m.id.thirdparty\", \"medium\": \"email\", \"address\": \"{}\" }}, \"password\":\"{}\"}}",
                                username, password
                        )
                        .as_bytes(),
                )
                .unwrap();
	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|data| {
				dst.extend_from_slice(data);
				Ok(data.len())
			})
			.unwrap();
		transfer.perform().unwrap();
	}
	serde_json::from_str(dbg!(String::from_utf8(dst).as_ref()).unwrap()).unwrap()
}

pub fn create_room(homeserver: &str, access_token: &str) -> CreateRoomResponse {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/createRoom?access_token={}",
				homeserver, access_token
			)
			.as_ref(),
		)
		.unwrap();
	handle
		.post_fields_copy(format!("{{\"room_alias\":\"\"}}").as_bytes())
		.unwrap();
	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|data| {
				dst.extend_from_slice(data);
				Ok(data.len())
			})
			.unwrap();
		transfer.perform().unwrap();
	}
	serde_json::from_str(String::from_utf8(dst).as_ref().unwrap()).unwrap()
}

pub fn invite(homeserver: &str, access_token: &str, room_id: &str, user_id: &str) {
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/rooms/{}/invite?access_token={}",
				homeserver, room_id, access_token
			)
			.as_ref(),
		)
		.unwrap();
	handle
		.post_fields_copy(format!("{{\"user_id\":\"{}\"}}", user_id).as_bytes())
		.unwrap();
	handle.perform().unwrap();
}

pub fn send_message(homeserver: &str, access_token: &str, room_id: &str, body: &str) {
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/rooms/{}/send/m.room.message?access_token={}",
				homeserver, room_id, access_token
			)
			.as_ref(),
		)
		.unwrap();
	handle
		.post_fields_copy(format!("{{\"msgtype\":\"m.text\",\"body\":\"{}\"}}", body).as_bytes())
		.unwrap();
	handle.perform().unwrap();
}
