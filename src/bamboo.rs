use crate::{
	error,
	Result,
};
use curl::easy::Easy;
use futures::stream::FuturesUnordered;
use rayon::prelude::*;
use serde::*;
use serde::{
	Deserialize,
	Serialize,
};
use snafu::ResultExt;
use std::collections::HashMap;
use std::io::{
	stdout,
	Write,
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmployeesDirectoryResponse {
	employees: Vec<EmployeesDirectoryInnerResponse>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmployeesDirectoryInnerResponse {
	id: String,
	display_name: Option<String>,
	first_name: Option<String>,
	last_name: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct EmployeeResponse {
	id: String,
	#[serde(rename = "customRiotID")]
	riot_id: Option<String>,
	#[serde(rename = "customGithub")]
	github: Option<String>,
}

const BASE_URL: &'static str =
	"https://api.bamboohr.com/api/gateway.php/parity/v1";

fn get_employees_directory(
	access_token: &str,
) -> Result<EmployeesDirectoryResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url(&format!("{}/employees/directory", BASE_URL))
		.or_else(error::map_curl_error)?;
	handle
		.username(access_token)
		.or_else(error::map_curl_error)?;
	handle.password("x").or_else(error::map_curl_error)?;
	handle
		.accept_encoding("application/json")
		.or_else(error::map_curl_error)?;
	let mut headers = curl::easy::List::new();
	headers
		.append("accept: application/json")
		.or_else(error::map_curl_error)?;
	handle
		.http_headers(headers)
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

fn get_employee(
	access_token: &str,
	employee_id: &str,
) -> Result<EmployeeResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url(
			format!(
				"{}/employees/{}/?fields=customGithub%2CcustomRiotID",
				BASE_URL, employee_id
			)
			.as_ref(),
		)
		.or_else(error::map_curl_error)?;
	handle
		.username(access_token)
		.or_else(error::map_curl_error)?;
	handle.password("x").or_else(error::map_curl_error)?;
	handle
		.accept_encoding("application/json")
		.or_else(error::map_curl_error)?;
	let mut headers = curl::easy::List::new();
	headers
		.append("accept: application/json")
		.or_else(error::map_curl_error)?;
	handle
		.http_headers(headers)
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

pub fn github_to_matrix(access_token: &str) -> Result<HashMap<String, String>> {
	get_employees_directory(&access_token).map(|response| {
		response
			.employees
			.into_iter()
			.filter_map(|EmployeesDirectoryInnerResponse { id, .. }| {
				get_employee(&access_token, &id).ok().and_then(
					|EmployeeResponse {
					     github, riot_id, ..
					 }| {
						if github.is_some() && riot_id.is_some() {
							Some((github.unwrap(), riot_id.unwrap()))
						} else {
							None
						}
					},
				)
			})
			.collect::<HashMap<String, String>>()
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_get_employees_directory() {
		dotenv::dotenv().ok();
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
		let res = get_employees_directory(&bamboo_token).map(|response| {
			response
				.employees
				.into_iter()
				.filter(|empl| empl.last_name == Some("Mark".to_owned()))
				.collect::<Vec<_>>()
		});
		dbg!(res);
	}

	#[test]
	fn test_get_employee() {
		dotenv::dotenv().ok();
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
		dbg!(get_employee(&bamboo_token, "110"));
	}
}
