/*
Processbot looks for configuration variables in `.env` in the root directory. Eg. `MATRIX_USER=annoying_bot@parity.io`.

`PRIVATE_KEY_PATH`: Path to the private key associated with the installed Processbot app.

`GITHUB_APP_ID`: App ID associated with the installed Processbot app.

`DB_PATH`: Path to an existing `rocksdb` database or that path at which a database will be created.

`MAIN_TICK_SECS`: Seconds between cycles of the main bot loop.

`BAMBOO_TOKEN`: API Key used to access the BambooHR API.

`BAMBOO_TICK_SECS`: Seconds between updating data pulled from the BambooHR API. This can take some time and is likely to change only infrequently, so the value should be larger than `MAIN_TICK_SECS`.

`MATRIX_SILENT`: If `true`, do not send Matrix notifications.

`MATRIX_HOMESERVER`: Matrix homeserver.

`MATRIX_ACCESS_TOKEN`: Matrix access token.

`MATRIX_DEFAULT_CHANNEL_ID`: ID of a channel the bot should use when specific project details are unavailable.

`STATUS_FAILURE_PING`: Seconds between notifications that a pull request has failed checks, sent privately to the pull request author, via Matrix.

`ISSUE_NOT_ASSIGNED_TO_PR_AUTHOR_PING`: Seconds between notifications that the issue relevant to a pull request has not been assigned to the author of the pull
request, sent privately to the issue assignee and project owner, then publicly to the project room, via Matrix.

`NO_PROJECT_AUTHOR_IS_CORE_PING`: Seconds between notifications that a pull request opened by a core developer has no project attached, sent privately to the
pull request author or publicly to the default channel if the author's Matrix handle cannot be found.

`NO_PROJECT_AUTHOR_IS_CORE_CLOSE_PR`: Seconds before closing a pull request opened by a core developer that has no project attached.

`NO_PROJECT_AUTHOR_UNKNOWN_CLOSE_PR`: Seconds before closing a pull request opened by an external developer that has no project attached.

`PROJECT_CONFIRMATION_TIMEOUT`: Seconds before reverting an unconfirmed change of project by a non-whitelisted developer (currently unimplemented).

`MIN_REVIEWERS`: Minimum number of reviewers needed before a pull request can be accepted.

`REVIEW_REQUEST_PING`: Seconds between notifications requesting reviews on a pull request, sent publicly to the relevant project room, via Matrix.

`PRIVATE_REVIEW_REMINDER_PING`: Seconds between notifications reminding a reviewer to review a pull request, sent privately to the reviewer, via Matrix.

`PUBLIC_REVIEW_REMINDER_PING`: Seconds between notifications reminding a reviewer to review a pull request, sent publicly to the relevant project room, via Matrix.

`TEST_REPO_NAME`: Name of a Github repository to be used for testing.

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
	pub bamboo_token: String,
	pub private_key: Vec<u8>,
	pub matrix_homeserver: String,
	pub matrix_access_token: String,
	pub matrix_default_channel_id: String,
	pub main_tick_secs: u64,
	pub bamboo_tick_secs: u64,
	/// if true then matrix notifications will not be sent
	pub matrix_silent: bool,
	pub gitlab_hostname: String,
	pub gitlab_project: String,
	pub gitlab_job_name: String,
	pub gitlab_private_token: String,
	pub github_app_id: usize,
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
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
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
		let bamboo_tick_secs = dotenv::var("BAMBOO_TICK_SECS")
			.expect("BAMBOO_TICK_SECS")
			.parse::<u64>()
			.expect("parse BAMBOO_TICK_SECS");
		let matrix_silent = dotenv::var("MATRIX_SILENT")
			.expect("MATRIX_SILENT")
			.parse::<bool>()
			.expect("failed parsing MATRIX_SILENT");

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

		let github_app_id = dotenv::var("GITHUB_APP_ID")
			.unwrap()
			.parse::<usize>()
			.expect("GITHUB_APP_ID");

		Self {
			environment,
			test_repo,
			installation_login,
			webhook_secret,
			webhook_port,
			db_path,
			bamboo_token,
			private_key,
			matrix_homeserver,
			matrix_access_token,
			matrix_default_channel_id,
			main_tick_secs,
			bamboo_tick_secs,
			matrix_silent,
			gitlab_hostname,
			gitlab_project,
			gitlab_job_name,
			gitlab_private_token,
			github_app_id,
		}
	}
}

#[derive(Debug, Clone)]
pub struct BotConfig {
	/// seconds between pings
	pub status_failure_ping: u64,
	/// seconds between pings
	pub issue_not_addressed_ping: u64,
	/// seconds between pings
	pub issue_not_assigned_to_pr_author_ping: u64,
	/// seconds between pings
	pub no_project_author_is_core_ping: u64,
	/// seconds before pr gets closed
	pub no_project_author_is_core_close_pr: u64,
	/// seconds before pr gets closed
	pub no_project_author_unknown_close_pr: u64,
	/// seconds before unconfirmed change gets reverted
	pub project_confirmation_timeout: u64,
	/// seconds between pings
	pub review_request_ping: u64,
	/// seconds between pings
	pub private_review_reminder_ping: u64,
	/// seconds between pings
	pub public_review_reminder_ping: u64,
	/// seconds before public review reminders begin
	pub public_review_reminder_delay: u64,
	/// mininum number of reviewers
	pub min_reviewers: usize,
	/// name of repo for issues without a project
	pub core_sorting_repo_name: String,
	/// matrix room id for sending app logs
	pub logs_room_id: String,
}

impl BotConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();
		Self {
			status_failure_ping: dotenv::var("STATUS_FAILURE_PING")
				.expect("STATUS_FAILURE_PING")
				.parse::<u64>()
				.expect("failed parsing STATUS_FAILURE_PING"),

			issue_not_addressed_ping: dotenv::var("ISSUE_NOT_ADDRESSED_PING")
				.expect("ISSUE_NOT_ADDRESSED_PING")
				.parse::<u64>()
				.expect("failed parsing ISSUE_NOT_ADDRESSED_PING"),

			issue_not_assigned_to_pr_author_ping: dotenv::var(
				"ISSUE_NOT_ASSIGNED_TO_PR_AUTHOR_PING",
			)
			.expect("ISSUE_NOT_ASSIGNED_TO_PR_AUTHOR_PING")
			.parse::<u64>()
			.expect("failed parsing ISSUE_NOT_ASSIGNED_TO_PR_AUTHOR_PING"),

			no_project_author_is_core_ping: dotenv::var(
				"NO_PROJECT_AUTHOR_IS_CORE_PING",
			)
			.expect("NO_PROJECT_AUTHOR_IS_CORE_PING")
			.parse::<u64>()
			.expect("failed parsing NO_PROJECT_AUTHOR_IS_CORE_PING"),

			no_project_author_is_core_close_pr: dotenv::var(
				"NO_PROJECT_AUTHOR_IS_CORE_CLOSE_PR",
			)
			.expect("NO_PROJECT_AUTHOR_IS_CORE_CLOSE_PR")
			.parse::<u64>()
			.expect("failed parsing NO_PROJECT_AUTHOR_IS_CORE_CLOSE_PR"),

			no_project_author_unknown_close_pr: dotenv::var(
				"NO_PROJECT_AUTHOR_UNKNOWN_CLOSE_PR",
			)
			.expect("NO_PROJECT_AUTHOR_UNKNOWN_CLOSE_PR")
			.parse::<u64>()
			.expect("failed parsing NO_PROJECT_AUTHOR_UNKNOWN_CLOSE_PR"),

			project_confirmation_timeout: dotenv::var(
				"PROJECT_CONFIRMATION_TIMEOUT",
			)
			.expect("PROJECT_CONFIRMATION_TIMEOUT")
			.parse::<u64>()
			.expect("failed parsing PROJECT_CONFIRMATION_TIMEOUT"),

			review_request_ping: dotenv::var("REVIEW_REQUEST_PING")
				.expect("REVIEW_REQUEST_PING")
				.parse::<u64>()
				.expect("failed parsing REVIEW_REQUEST_PING"),

			private_review_reminder_ping: dotenv::var(
				"PRIVATE_REVIEW_REMINDER_PING",
			)
			.expect("PRIVATE_REVIEW_REMINDER_PING")
			.parse::<u64>()
			.expect("failed parsing PRIVATE_REVIEW_REMINDER_PING"),

			public_review_reminder_ping: dotenv::var(
				"PUBLIC_REVIEW_REMINDER_PING",
			)
			.expect("PUBLIC_REVIEW_REMINDER_PING")
			.parse::<u64>()
			.expect("failed parsing PUBLIC_REVIEW_REMINDER_PING"),

			public_review_reminder_delay: dotenv::var(
				"PUBLIC_REVIEW_REMINDER_DELAY",
			)
			.expect("PUBLIC_REVIEW_REMINDER_DELAY")
			.parse::<u64>()
			.expect("failed parsing PUBLIC_REVIEW_REMINDER_DELAY"),

			min_reviewers: dotenv::var("MIN_REVIEWERS")
				.expect("MIN_REVIEWERS")
				.parse::<usize>()
				.expect("failed parsing MIN_REVIEWERS"),

			core_sorting_repo_name: dotenv::var("CORE_SORTING_REPO_NAME")
				.expect("CORE_SORTING_REPO_NAME"),

			logs_room_id: dotenv::var("LOGS_ROOM_ID").expect("LOGS_ROOM_ID"),
		}
	}
}
