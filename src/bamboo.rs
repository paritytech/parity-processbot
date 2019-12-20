use curl::easy::Easy;
use serde::*;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::io::{stdout, Write};

use crate::{error, Result};

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
}

pub fn get_employees_directory(access_token: &str) -> Result<EmployeesDirectoryResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
		.url("https://api.bamboohr.com/api/gateway.php/parity/v1/employees/directory")
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
	serde_json::from_str(String::from_utf8(dst).as_ref().unwrap()).context(error::Json)
}

pub fn get_employee(access_token: &str, employee_id: &str) -> Result<EmployeeResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle
                .url(format!("https://api.bamboohr.com/api/gateway.php/parity/v1/employees/{}/?fields=customRiotID", employee_id).as_ref())
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
	serde_json::from_str(dbg!(String::from_utf8(dst).as_ref().unwrap())).context(error::Json)
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
