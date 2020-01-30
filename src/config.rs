#[derive(Debug, Clone)]
pub struct MainConfig {
	pub db_path: String,
	pub bamboo_token: String,
	pub private_key: Vec<u8>,
	pub matrix_homeserver: String,
	pub matrix_user: String,
	pub matrix_password: String,
	pub matrix_default_channel_id: String,
	pub main_tick_secs: u64,
	pub bamboo_tick_secs: u64,
}

impl MainConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();

		let db_path = dotenv::var("DB_PATH").expect("DB_PATH");
		let bamboo_token = dotenv::var("BAMBOO_TOKEN").expect("BAMBOO_TOKEN");
		let matrix_homeserver =
			dotenv::var("MATRIX_HOMESERVER").expect("MATRIX_HOMESERVER");
		let matrix_user = dotenv::var("MATRIX_USER").expect("MATRIX_USER");
		let matrix_password =
			dotenv::var("MATRIX_PASSWORD").expect("MATRIX_PASSWORD");
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

		let private_key_path =
			dotenv::var("PRIVATE_KEY_PATH").expect("PRIVATE_KEY_PATH");
		let private_key = std::fs::read(&private_key_path)
			.expect("Couldn't find private key.");

		Self {
			db_path,
			bamboo_token,
			private_key,
			matrix_homeserver,
			matrix_user,
			matrix_password,
			matrix_default_channel_id,
			main_tick_secs,
			bamboo_tick_secs,
		}
	}
}

#[derive(Debug, Clone)]
pub struct BotConfig {
	/// seconds between pings
	pub status_failure_ping: u64,
	/// seconds between pings
	pub issue_not_assigned_to_pr_author_ping: u64,
	/// seconds between pings
	pub no_project_author_is_core_ping: u64,
	/// seconds before pr gets closed
	pub no_project_author_is_core_close_pr: u64,
	/// seconds before pr gets closed
	pub no_project_author_not_core_close_pr: u64,
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
	/// matrix room id to be used when missing project details
	pub fallback_room_id: String, // TODO remove in favour of Config's default_matrix_room_id
	/// name of repo for issues without a project
	pub core_sorting_repo_name: String,
	/// name of project column to which new issues should be attached
	pub project_backlog_column_name: String,
}

impl BotConfig {
	pub fn from_env() -> Self {
		dotenv::dotenv().ok();
		Self {
			status_failure_ping: dotenv::var("STATUS_FAILURE_PING")
				.expect("STATUS_FAILURE_PING")
				.parse::<u64>()
				.expect("failed parsing STATUS_FAILURE_PING"),

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

			no_project_author_not_core_close_pr: dotenv::var(
				"NO_PROJECT_AUTHOR_NOT_CORE_CLOSE_PR",
			)
			.expect("NO_PROJECT_AUTHOR_NOT_CORE_CLOSE_PR")
			.parse::<u64>()
			.expect("failed parsing NO_PROJECT_AUTHOR_NOT_CORE_CLOSE_PR"),

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

			fallback_room_id: dotenv::var("FALLBACK_ROOM_ID")
				.expect("FALLBACK_ROOM_ID"),

			core_sorting_repo_name: dotenv::var("CORE_SORTING_REPO_NAME")
				.expect("CORE_SORTING_REPO_NAME"),

			project_backlog_column_name: dotenv::var(
				"PROJECT_BACKLOG_COLUMN_NAME",
			)
			.expect("PROJECT_BACKLOG_COLUMN_NAME"),
		}
	}
}
