use crate::{error, Result};
use curl::easy::Easy;
use serde::Deserialize;
use snafu::ResultExt;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmployeesDirectoryResponse {
	employees: Vec<EmployeesDirectoryInnerResponse>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmployeesDirectoryInnerResponse {
	id: String,
}

#[derive(Deserialize, Debug)]
pub struct EmployeeResponse {
	id: String,
	#[serde(rename = "customRiotID")]
	riot_id: Option<String>,
	#[serde(rename = "customGithub")]
	github: Option<String>,
}

const BASE_URL: &str = "https://api.bamboohr.com/api/gateway.php/parity/v1";

/// Return data for all employees.
fn get_employees_directory(
	access_token: &str,
) -> Result<EmployeesDirectoryResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle.url(&format!("{}/employees/directory", BASE_URL))?;
	handle.username(access_token)?;
	handle.password("x")?;
	handle.accept_encoding("application/json")?;
	let mut headers = curl::easy::List::new();
	headers.append("accept: application/json")?;
	handle.http_headers(headers)?;
	{
		let mut transfer = handle.transfer();
		transfer.write_function(|data| {
			dst.extend_from_slice(data);
			Ok(data.len())
		})?;
		transfer.perform()?;
	}
	String::from_utf8(dst)
		.context(error::Utf8)
		.and_then(|s| serde_json::from_str(&s).context(error::Json))
}

/// Return private data for a single employee.
fn get_employee(
	access_token: &str,
	employee_id: &str,
) -> Result<EmployeeResponse> {
	let mut dst = Vec::new();
	let mut handle = Easy::new();
	handle.url(
		format!(
			"{}/employees/{}/?fields=customGithub%2CcustomRiotID",
			BASE_URL, employee_id
		)
		.as_ref(),
	)?;
	handle.username(access_token)?;
	handle.password("x")?;
	handle.accept_encoding("application/json")?;
	let mut headers = curl::easy::List::new();
	headers.append("accept: application/json")?;
	handle.http_headers(headers)?;
	{
		let mut transfer = handle.transfer();
		transfer.write_function(|data| {
			dst.extend_from_slice(data);
			Ok(data.len())
		})?;
		transfer.perform()?;
	}
	serde_json::from_str(String::from_utf8(dst).as_ref().unwrap())
		.context(error::Json)
}

/// Serially fetch and return data for each employee.  This takes a long time as it must send an
/// individual request for each employee.
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
	#[ignore]
	fn test_get_employee() {
		dotenv::dotenv().ok();
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
		dbg!(get_employee(&bamboo_token, "110")).unwrap();
	}
}
