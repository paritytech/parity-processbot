/*
`PRIVATE_KEY_PATH`: Path to the private key associated with the installed Processbot app.

`GITHUB_APP_ID`: App ID associated with the installed Processbot app.

`DB_PATH`: Path to an existing `rocksdb` database or that path at which a database will be created.

`GITLAB_HOSTNAME`: Hostname of the Gitlab server used for burn-in deployment related CI jobs.

`GITLAB_PROJECT`: Name of the project in Gitlab where CI jobs for burn-in deployments can be found.

`GITLAB_PRIVATE_TOKEN`: Authentication token for the Gitlab server at GITLAB_HOSTNAME.
*/

#[derive(Debug, Clone)]
pub struct MainConfig {
	pub environment: String,
	pub test_repo: String,
	pub installation_login: String,
	pub webhook_secret: String,
	pub webhook_port: String,
	pub db_path: String,
	pub private_key: Vec<u8>,
	pub matrix_homeserver: String,
	pub matrix_access_token: String,
	pub matrix_default_channel_id: String,
	pub main_tick_secs: u64,
	/// if true then matrix notifications will not be sent
	pub matrix_silent: bool,
	pub gitlab_hostname: String,
	pub gitlab_project: String,
	pub gitlab_job_name: String,
	pub gitlab_private_token: String,
}

impl MainConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		let environment = dotenv::var("ENVIRONMENT").expect("ENVIRONMENT");
		let test_repo = dotenv::var("TEST_REPO_NAME").expect("TEST_REPO_NAME");
		let installation_login =
			dotenv::var("INSTALLATION_LOGIN").expect("INSTALLATION_LOGIN");
		let webhook_secret =
			dotenv::var("WEBHOOK_SECRET").expect("WEBHOOK_SECRET");
		let webhook_port = dotenv::var("WEBHOOK_PORT").expect("WEBHOOK_PORT");
		let db_path = dotenv::var("DB_PATH").expect("DB_PATH");
		let matrix_homeserver =
			dotenv::var("MATRIX_HOMESERVER").expect("MATRIX_HOMESERVER");
		let matrix_access_token =
			dotenv::var("MATRIX_ACCESS_TOKEN").expect("MATRIX_ACCESS_TOKEN");
		let matrix_default_channel_id =
			dotenv::var("MATRIX_DEFAULT_CHANNEL_ID")
				.expect("MATRIX_DEFAULT_CHANNEL_ID");
		let main_tick_secs = dotenv::var("MAIN_TICK_SECS")
			.expect("MAIN_TICK_SECS")
			.parse::<u64>()
			.expect("parse MAIN_TICK_SECS");

		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		let gitlab_hostname =
			dotenv::var("GITLAB_HOSTNAME").expect("GITLAB_HOSTNAME");
		let gitlab_project =
			dotenv::var("GITLAB_PROJECT").expect("GITLAB_PROJECT");
		let gitlab_job_name =
			dotenv::var("GITLAB_JOB_NAME").expect("GITLAB_JOB_NAME");
		let gitlab_private_token =
			dotenv::var("GITLAB_PRIVATE_TOKEN").expect("GITLAB_PRIVATE_TOKEN");

		Self {
			environment,
			test_repo,
			installation_login,
			webhook_secret,
			webhook_port,
			db_path,
			private_key,
			matrix_homeserver,
			matrix_access_token,
			matrix_default_channel_id,
			main_tick_secs,
			matrix_silent,
			gitlab_hostname,
			gitlab_project,
			gitlab_job_name,
			gitlab_private_token,
		}
	}
}
