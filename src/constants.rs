/*
 * Ping periods measured in seconds
 */
pub const STATUS_FAILURE_PING_PERIOD: u64 = 3600 * 24;
pub const ISSUE_NOT_ASSIGNED_PING_PERIOD: u64 = 3600 * 24;
pub const ISSUE_NO_PROJECT_CORE_PING_PERIOD: u64 = 3600 * 8;
pub const ISSUE_NO_PROJECT_NON_CORE_PING_PERIOD: u64 = 60 * 15;
pub const ISSUE_NO_PROJECT_ACTION_AFTER_NPINGS: u64 = 9;

pub const FALLBACK_ROOM_ID: &'static str =
	"!aenJixaHcSKbJOWxYk:matrix.parity.io";
pub const ISSUE_MUST_EXIST_MESSAGE: &'static str =
	"Every pull request must address an issue.";
pub const ISSUE_NO_PROJECT_MESSAGE: &'static str =
	"{1} needs to be attached to a project or it will be closed.";
pub const ISSUE_ASSIGNEE_NOTIFICATION: &'static str = "{1} addressing {2} has been opened by {3}. Please reassign the issue or close the pull request.";
pub const REQUESTING_REVIEWS_MESSAGE: &'static str =
	"{1} is in need of reviewers.";
pub const STATUS_FAILURE_NOTIFICATION: &'static str =
	"{1} has failed status checks.";
pub const REQUEST_DELEGATED_REVIEW_MESSAGE: &'static str =
	"{1} needs your review in the next 72 hours, as you are the delegated reviewer.";
pub const REQUEST_OWNER_REVIEW_MESSAGE: &'static str =
	"{1} needs your review in the next 72 hours, as you are the project owner.";

pub const CORE_SORTING_REPO: &'static str = "core-sorting";
