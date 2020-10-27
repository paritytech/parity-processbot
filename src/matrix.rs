use curl::easy::Easy;
use regex::Regex;
use serde::Deserialize;
use snafu::ResultExt;

use crate::{error, Result};

#[derive(Deserialize, Debug)]
pub struct LoginResponse {
	pub access_token: String,
}

#[derive(Deserialize, Debug)]
pub struct CreateRoomResponse {
	pub room_id: String,
}

pub fn login(
	homeserver: &str,
	username: &str,
	password: &str,
) -> Result<LoginResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url(format!("{}/_matrix/client/r0/login", homeserver).as_ref())
		.or_else(error::map_curl_error)?;
	handle
		.post_fields_copy(
			serde_json::json!({ "type": "m.login.password", "identifier": { "type": "m.id.thirdparty", "medium": "email", "address": username }, "password": password })
				.to_string()
				.as_bytes(),
		)
		.or_else(error::map_curl_error)?;
	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|data| {
				dst.extend_from_slice(data);
				Ok(data.len())
			})
			.or_else(error::map_curl_error)?;
		transfer.perform().or_else(error::map_curl_error)?;
	}
	dbg!(String::from_utf8(dst)
		.context(error::Utf8)
		.and_then(|s| serde_json::from_str(&s).context(error::Json)))
}

pub fn sync(homeserver: &str, access_token: &str) -> Result<String> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/sync?access_token={}",
				homeserver, access_token
			)
			.as_ref(),
		)
		.or_else(error::map_curl_error)?;
	handle.get(false).or_else(error::map_curl_error)?;
	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|data| {
				dst.extend_from_slice(data);
				Ok(data.len())
			})
			.or_else(error::map_curl_error)?;
		transfer.perform().or_else(error::map_curl_error)?;
	}
	String::from_utf8(dst).context(error::Utf8)
}

pub fn create_room(
	homeserver: &str,
	access_token: &str,
) -> Result<CreateRoomResponse> {
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
		.or_else(error::map_curl_error)?;
	handle
		.post_fields_copy(
			serde_json::json!({ "room_alias": "" })
				.to_string()
				.as_bytes(),
		)
		.or_else(error::map_curl_error)?;
	{
		let mut transfer = handle.transfer();
		transfer
			.write_function(|data| {
				dst.extend_from_slice(data);
				Ok(data.len())
			})
			.or_else(error::map_curl_error)?;
		transfer.perform().or_else(error::map_curl_error)?;
	}
	serde_json::from_str(String::from_utf8(dst).as_ref().unwrap())
		.context(error::Json)
}

pub fn invite(
	homeserver: &str,
	access_token: &str,
	room_id: &str,
	user_id: &str,
) -> Result<()> {
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/rooms/{}/invite?access_token={}",
				homeserver, room_id, access_token
			)
			.as_ref(),
		)
		.or_else(error::map_curl_error)?;
	handle
		.post_fields_copy(
			serde_json::json!({ "user_id": user_id })
				.to_string()
				.as_bytes(),
		)
		.or_else(error::map_curl_error)?;
	handle.perform().or_else(error::map_curl_error)
}

pub fn send_message(
	homeserver: &str,
	access_token: &str,
	room_id: &str,
	body: &str,
) -> Result<()> {
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/rooms/{}/send/m.room.message?access_token={}",
				homeserver, room_id, access_token
			)
			.as_ref(),
		)
		.or_else(error::map_curl_error)?;
	handle
		.post_fields_copy(
			serde_json::json!({ "msgtype": "m.text", "body": body })
				.to_string()
				.as_bytes(),
		)
		.or_else(error::map_curl_error)?;
	handle.perform().or_else(error::map_curl_error)
}

pub fn send_html_message(
	homeserver: &str,
	access_token: &str,
	room_id: &str,
	body: &str,
) -> Result<()> {
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/_matrix/client/r0/rooms/{}/send/m.room.message?access_token={}",
				homeserver, room_id, access_token
			)
			.as_ref(),
		)
		.or_else(error::map_curl_error)?;
	handle
		.post_fields_copy(
			serde_json::json!(
				{
					"msgtype": "m.text",
					"format": "org.matrix.custom.html",
					"body": "",
					"formatted_body": body
				}
			)
			.to_string()
			.as_bytes(),
		)
		.or_else(error::map_curl_error)?;
	handle.perform().or_else(error::map_curl_error)
}

/// If the pattern is recognised, return the full matrix id.
/// Otherwise, return None.
pub fn parse_id(matrix_id: &str) -> Option<String> {
	let full_handle = Regex::new(r"^@[\w]+:matrix.parity.io$").unwrap();
	let no_at = Regex::new(r"^[\w]+:matrix.parity.io$").unwrap();
	let no_domain = Regex::new(r"^@[\w]+$").unwrap();
	let name_only = Regex::new(r"^[\w]+$").unwrap();
	if full_handle.is_match(matrix_id) {
		Some(format!("{}", matrix_id))
	} else if no_at.is_match(matrix_id) {
		Some(format!("@{}", matrix_id))
	} else if no_domain.is_match(matrix_id) {
		Some(format!("{}:matrix.parity.io", matrix_id))
	} else if name_only.is_match(matrix_id) {
		Some(format!("@{}:matrix.parity.io", matrix_id))
	} else {
		None
	}
}

/*
/// If the pattern is recognised, return the name.
/// Otherwise, return None.
pub fn parse_id_name(matrix_id: &str) -> Option<String> {
	let full_handle = Regex::new(r"^@[\w]+:matrix.parity.io$").unwrap();
	let no_at = Regex::new(r"^[\w]+:matrix.parity.io$").unwrap();
	let no_domain = Regex::new(r"^@[\w]+$").unwrap();
	let name_only = Regex::new(r"^[\w]+$").unwrap();
	if full_handle.is_match(matrix_id) {
		Some(format!("{}", matrix_id))
	} else if no_at.is_match(matrix_id) {
		Some(format!("@{}", matrix_id))
	} else if no_domain.is_match(matrix_id) {
		Some(format!("{}:matrix.parity.io", matrix_id))
	} else if name_only.is_match(matrix_id) {
		Some(format!("@{}:matrix.parity.io", matrix_id))
	} else {
		None
	}
}
*/

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_matrix_id() {
		let correct = Some("@joseph:matrix.parity.io".to_owned());
		assert_eq!(parse_id("@joseph:matrix.parity.io"), correct);
		assert_eq!(parse_id("joseph:matrix.parity.io"), correct);
		assert_eq!(parse_id("@joseph"), correct);
		assert_eq!(parse_id("joseph"), correct);
		assert_eq!(parse_id("@joseph:matrix.arsenal.io"), None);
	}
}
