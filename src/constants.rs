// NOTE: If you add a new command, put it in the BOT_COMMANDS array
pub const AUTO_MERGE_REQUEST: &str = "bot merge";
pub const AUTO_MERGE_FORCE: &str = "bot merge force";
pub const AUTO_MERGE_CANCEL: &str = "bot merge cancel";
pub const REBASE: &str = "bot rebase";
pub const BURNIN_REQUEST: &str = "bot burnin";
pub const COMPARE_RELEASE_REQUEST: &str = "bot compare substrate";
// NOTE: Put all commands here, otherwise the bot will not detect them
pub const BOT_COMMANDS: [&str; 6] = [
	AUTO_MERGE_REQUEST,
	AUTO_MERGE_FORCE,
	AUTO_MERGE_CANCEL,
	REBASE,
	BURNIN_REQUEST,
	COMPARE_RELEASE_REQUEST,
];

pub const SUBSTRATE_TEAM_LEADS_GROUP: &str = "substrateteamleads";

pub const CORE_DEVS_GROUP: &str = "core-devs";

pub const PROCESS_FILE: &str = "Process.json";

pub const MAIN_REPO_FOR_STAGING: &str = "main-for-processbot-staging";
