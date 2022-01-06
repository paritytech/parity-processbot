use std::path::PathBuf;

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
	pub disable_org_check: bool,
	pub github_api_url: String,
	pub companion_status_settle_delay: u64,
	pub merge_command_delay: u64,
	pub core_devs_team: String,
	pub team_leads_team: String,
	pub github_source_prefix: String,
	pub github_source_suffix: String,
}

impl MainConfig {
	pub fn from_env() -> Self {
		let repo_root = PathBuf::from(
			std::env::var("CARGO_MANIFEST_DIR")
			.expect("CARGO_MANIFEST_DIR is not set, please run the application through cargo")
		);

		dotenv::dotenv().ok();

		let installation_login =
			dotenv::var("INSTALLATION_LOGIN").expect("INSTALLATION_LOGIN");
		let webhook_secret =
			dotenv::var("WEBHOOK_SECRET").expect("WEBHOOK_SECRET");
		let webhook_port = dotenv::var("WEBHOOK_PORT").expect("WEBHOOK_PORT");

		let db_path = dotenv::var("DB_PATH").expect("DB_PATH");
		let db_path = if db_path.starts_with('/') {
			PathBuf::from(db_path)
		} else {
			repo_root.join(db_path)
		};
		std::fs::create_dir_all(&db_path)
			.expect("Could not create database directory (DB_PATH)");

		let repos_path =
			dotenv::var("REPOSITORIES_PATH").expect("REPOSITORIES_PATH");
		let repos_path = if repos_path.starts_with('/') {
			PathBuf::from(repos_path)
		} else {
			repo_root.join(repos_path)
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

		let disable_org_check = dotenv::var("DISABLE_ORG_CHECK")
			.ok()
			.map(|value| match value.as_str() {
				"true" => true,
				"false" => false,
				_ => {
					panic!("DISABLE_ORG_CHECK should be \"true\" or \"false\"")
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

		let core_devs_team =
			dotenv::var("CORE_DEVS_TEAM").expect("CORE_DEVS_TEAM");
		let team_leads_team =
			dotenv::var("TEAM_LEADS_TEAM").expect("TEAM_LEADS_TEAM");

		Self {
			installation_login,
			webhook_secret,
			webhook_port,
			db_path,
			private_key,
			webhook_proxy_url,
			github_app_id,
			disable_org_check,
			github_api_url,
			merge_command_delay,
			companion_status_settle_delay,
			core_devs_team,
			team_leads_team,
			repos_path,
			github_source_prefix,
			github_source_suffix,
		}
	}
}
