use serde::Deserialize;

#[derive(Deserialize)]
pub struct JobInformation {
	pub build_allow_failure: Option<bool>,
}
