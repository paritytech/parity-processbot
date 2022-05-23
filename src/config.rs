use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone)]
pub struct MainConfig {
	pub installation_login: String,
	pub webhook_secret: String,
	pub webhook_port: String,
	pub db_path: PathBuf,
	pub repos_path: PathBuf,
	pub private_key: Vec<u8>,
	pub webhook_proxy_url: Option<String>,
	pub github_app_id: usize,
	pub disable_org_checks: bool,
	pub github_api_url: String,
	pub companion_status_settle_delay: u64,
	pub merge_command_delay: u64,
	pub github_source_prefix: String,
	pub github_source_suffix: String,
	pub gitlab_url: String,
	pub gitlab_access_token: String,
	pub dependency_update_configuration: HashMap<String, Vec<String>>,
}

impl MainConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		let root_dir = if dotenv::var("START_FROM_CWD").is_ok() {
			std::env::current_dir().expect("START_FROM_CWD was set, but it was not possible to get the current directory")
		} else {
			PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set, please run the application through cargo"))
		};

		let installation_login =
			dotenv::var("INSTALLATION_LOGIN").expect("INSTALLATION_LOGIN");
		let webhook_secret =
			dotenv::var("WEBHOOK_SECRET").expect("WEBHOOK_SECRET");
		let webhook_port = dotenv::var("WEBHOOK_PORT").expect("WEBHOOK_PORT");

		let db_path = dotenv::var("DB_PATH").unwrap();
		let db_path = if db_path.starts_with('/') {
			PathBuf::from(db_path)
		} else {
			root_dir.join(db_path)
		};
		std::fs::create_dir_all(&db_path)
			.expect("Could not create database directory (DB_PATH)");

		let repos_path =
			dotenv::var("REPOSITORIES_PATH").expect("REPOSITORIES_PATH");
		let repos_path = if repos_path.starts_with('/') {
			PathBuf::from(repos_path)
		} else {
			root_dir.join(repos_path)
		};
		std::fs::create_dir_all(&repos_path).expect(
			"Could not create repositories directory (REPOSITORIES_PATH)",
		);

		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		let webhook_proxy_url = dotenv::var("WEBHOOK_PROXY_URL").ok();
		let github_app_id = dotenv::var("GITHUB_APP_ID")
			.expect("GITHUB_APP_ID")
			.parse::<usize>()
			.expect("GITHUB_APP_ID should be a number");

		let disable_org_checks = dotenv::var("DISABLE_ORG_CHECKS")
			.ok()
			.map(|value| match value.as_str() {
				"true" => true,
				"false" => false,
				_ => {
					panic!("DISABLE_ORG_CHECKS should be \"true\" or \"false\"")
				}
			})
			.unwrap_or(false);

		let github_api_url = "https://api.github.com".to_owned();
		let github_source_prefix = dotenv::var("GITHUB_SOURCE_PREFIX")
			.unwrap_or_else(|_| "https://github.com".to_string());
		let github_source_suffix = dotenv::var("GITHUB_SOURCE_SUFFIX")
			.unwrap_or_else(|_| "".to_string());

		let merge_command_delay = 4096;

		let companion_status_settle_delay = 4096;

		let gitlab_url = dotenv::var("GITLAB_URL").unwrap();
		let gitlab_access_token = dotenv::var("GITLAB_ACCESS_TOKEN").unwrap();

		let dependency_update_configuration = {
			let mut dependency_update_configuration = HashMap::new();

			if let Some(raw_configuration) =
				dotenv::var("DEPENDENCY_UPDATE_CONFIGURATION").ok()
			{
				for token in raw_configuration.split(':') {
					let token_parsing_err_msg = format!(
						"$DEPENDENCY_UPDATE_CONFIGURATION segment \"{}\" should be of the form REPOSITORY=DEPENDENCY,DEPENDENCY,...",
						token
					);

					let mut token_parts = token.split('=');
					let repository =
						token_parts.next().expect(&token_parsing_err_msg);
					let dependencies =
						token_parts.next().expect(&token_parsing_err_msg);
					if token_parts.next().is_some() {
						panic!("{}", token_parsing_err_msg)
					}

					dependency_update_configuration.insert(
						repository.into(),
						dependencies.split(',').map(|dep| dep.into()).collect(),
					);
				}
			}

			dependency_update_configuration
		};
		log::info!(
			"dependency_update_configuration: {:?}",
			dependency_update_configuration
		);

		Self {
			installation_login,
			webhook_secret,
			webhook_port,
			db_path,
			private_key,
			webhook_proxy_url,
			github_app_id,
			disable_org_checks,
			github_api_url,
			merge_command_delay,
			companion_status_settle_delay,
			repos_path,
			github_source_prefix,
			github_source_suffix,
			gitlab_url,
			gitlab_access_token,
			dependency_update_configuration,
		}
	}
}
