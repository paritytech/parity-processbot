use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Organization {
	pub login: String,
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub url: Option<String>,
	pub repos_url: String,
	pub events_url: Option<String>,
	pub hooks_url: Option<String>,
	pub issues_url: Option<String>,
	pub members_url: Option<String>,
	pub public_members_url: Option<String>,
	pub avatar_url: Option<String>,
	pub description: Option<String>,
	pub name: Option<String>,
	pub company: Option<String>,
	pub blog: Option<String>,
	pub location: Option<String>,
	pub email: Option<String>,
	pub is_verified: Option<bool>,
	pub has_organization_projects: Option<bool>,
	pub has_repository_projects: Option<bool>,
	pub public_repos: Option<i64>,
	pub public_gists: Option<i64>,
	pub followers: Option<i64>,
	pub following: Option<i64>,
	pub html_url: Option<String>,
	pub created_at: Option<String>,
	#[serde(rename = "type")]
	pub type_field: Option<String>,
	pub total_private_repos: Option<i64>,
	pub owned_private_repos: Option<i64>,
	pub private_gists: Option<i64>,
	pub disk_usage: Option<i64>,
	pub collaborators: Option<i64>,
	pub billing_email: Option<String>,
	pub plan: Option<Plan>,
	pub default_repository_settings: Option<String>,
	pub members_can_create_repositories: Option<bool>,
	pub two_factor_requirement_enabled: Option<bool>,
	pub members_allowed_repository_creation_type: Option<String>,
	pub members_can_create_public_repositories: Option<bool>,
	pub members_can_create_private_repositories: Option<bool>,
	pub members_can_create_internal_repositories: Option<bool>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contents {
	#[serde(rename = "type")]
	pub contents_type: String,
	pub encoding: String,
	pub size: i64,
	pub name: String,
	pub path: String,
	pub content: String,
	pub sha: String,
	pub url: Option<String>,
	pub git_url: String,
	pub html_url: Option<String>,
	pub download_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
	AddedToProject,
	Assigned,
	Closed,
	ConvertedNoteToIssue,
	Demilestoned,
	HeadRefDeleted,
	HeadRefForcePushed,
	HeadRefRestored,
	Labeled,
	Locked,
	Mentioned,
	MarkedAsDuplicate,
	Merged,
	Milestoned,
	MovedColumnsInProject,
	Referenced,
	RemovedFromProject,
	Renamed,
	Reopened,
	ReviewDismissed,
	ReviewRequested,
	ReviewRequestRemoved,
	Subscribed,
	Transferred,
	Unassigned,
	Unlabeled,
	Unlocked,
	UnmarkedAsDuplicate,
	UserBlocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueEvent {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub url: Option<String>,
	pub actor: User,
	pub assignee: Option<User>,
	pub assignees: Option<Vec<User>>,
	pub assigner: Option<User>,
	pub labels: Option<Vec<Label>>,
	pub milestone: Option<Milestone>,
	pub project_card: Option<ProjectCard>,
	pub event: Option<Event>,
	pub commit_id: Option<String>,
	pub commit_url: Option<String>,
	pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCard {
	pub id: Option<i64>,
	pub url: Option<String>,
	pub project_id: Option<i64>,
	pub project_url: Option<String>,
	pub column_name: Option<String>,
	pub previous_column_name: Option<String>,
	pub column_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
	pub owner_url: Option<String>,
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub columns_url: Option<String>,
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub name: String,
	pub body: Option<String>,
	pub number: Option<i64>,
	pub state: Option<String>,
	pub creator: Option<User>,
	pub created_at: Option<chrono::DateTime<chrono::Utc>>,
	pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProjectCardContentType {
	Issue,
	PullRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectColumn {
	pub url: Option<String>,
	pub project_url: Option<String>,
	pub cards_url: Option<String>,
	pub id: i64,
	pub node_id: Option<String>,
	pub name: Option<String>,
	pub created_at: Option<chrono::DateTime<chrono::Utc>>,
	pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub name: String,
	pub slug: String,
	pub description: String,
	pub privacy: String,
	pub permission: String,
	pub members_url: String,
	pub repositories_url: String,
	pub members_count: i64,
	pub repos_count: i64,
	pub created_at: String,
	pub updated_at: String,
	pub organization: Organization,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
	pub name: String,
	pub space: i64,
	pub private_repos: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssuePullRequest {
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub diff_url: Option<String>,
	pub patch_url: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
	pub url: Option<String>,
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub html_url: Option<String>,
	pub diff_url: Option<String>,
	pub patch_url: Option<String>,
	pub issue_url: Option<String>,
	pub commits_url: Option<String>,
	pub review_comments_url: Option<String>,
	pub review_comment_url: Option<String>,
	pub comments_url: Option<String>,
	pub statuses_url: Option<String>,
	pub number: i64,
	pub state: Option<String>,
	pub locked: Option<bool>,
	pub title: Option<String>,
	pub user: User,
	pub body: Option<String>,
	pub labels: Option<Vec<Label>>,
	pub milestone: Option<Milestone>,
	pub active_lock_reason: Option<String>,
	pub created_at: Option<chrono::DateTime<chrono::Utc>>,
	pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
	pub closed_at: Option<String>,
	pub mergeable: Option<bool>,
	pub merged_at: Option<String>,
	pub merge_commit_sha: Option<String>,
	pub assignee: Option<User>,
	pub assignees: Option<Vec<User>>,
	pub requested_reviewers: Option<Vec<User>>,
	pub requested_teams: Option<Vec<RequestedTeam>>,
	pub head: Head,
	pub base: Base,
	#[serde(rename = "_links")]
	pub links: Option<Links>,
	pub author_association: Option<String>,
	pub draft: Option<bool>,
	#[serde(rename = "repo")]
	pub repository: Option<Repository>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Head {
	pub label: String,
	#[serde(rename = "ref")]
	pub ref_field: String,
	pub sha: String,
	pub user: User,
	pub repo: Option<Repository>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
	pub label: String,
	#[serde(rename = "ref")]
	pub ref_field: String,
	pub sha: String,
	pub user: User,
	pub repo: Option<Repository>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub html_url: Option<String>,
	pub user: User,
	pub body: Option<String>,
	pub commit_id: Option<String>,
	pub state: Option<ReviewState>,
	pub pull_request_url: Option<String>,
	pub submitted_at: Option<chrono::DateTime<chrono::Utc>>,
	#[serde(rename = "_links")]
	pub links: Option<Links>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestedReviewers {
	pub users: Vec<User>,
	pub teams: Vec<Team>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
	pub number: i64,
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub html_url: Option<String>,
	pub user: User,
	pub body: Option<String>,
	pub title: Option<String>,
	pub state: Option<String>,
	pub labels: Vec<Label>,
	pub assignee: Option<User>,
	pub assignees: Vec<User>,
	pub milestone: Option<Milestone>,
	pub locked: Option<bool>,
	pub active_lock_reason: Option<String>,
	pub pull_request: Option<IssuePullRequest>,
	pub created_at: String,
	pub updated_at: String,
	pub closed_at: Option<String>,
	pub repository: Option<Repository>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
	pub login: String,
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub avatar_url: String,
	pub gravatar_id: String,
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub followers_url: String,
	pub following_url: String,
	pub gists_url: String,
	pub starred_url: String,
	pub subscriptions_url: String,
	pub organizations_url: String,
	pub repos_url: String,
	pub events_url: String,
	pub received_events_url: String,
	#[serde(rename = "type")]
	pub type_field: String,
	pub site_admin: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub url: Option<String>,
	pub name: String,
	pub description: Option<String>,
	pub color: String,
	pub default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Milestone {
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub labels_url: Option<String>,
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub number: Option<i64>,
	pub state: Option<String>,
	pub title: String,
	pub description: Option<String>,
	pub creator: Option<User>,
	pub open_issues: Option<i64>,
	pub closed_issues: Option<i64>,
	pub created_at: Option<String>,
	pub updated_at: Option<String>,
	pub closed_at: Option<String>,
	pub due_on: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestedTeam {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub name: String,
	pub slug: String,
	pub description: Option<String>,
	pub privacy: String,
	pub permission: String,
	pub members_url: String,
	pub repositories_url: String,
	pub parent: ::serde_json::Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repository {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub name: String,
	pub full_name: String,
	pub owner: User,
	pub private: bool,
	pub html_url: Option<String>,
	pub description: Option<String>,
	pub fork: bool,
	pub url: Option<String>,
	pub archive_url: String,
	pub assignees_url: String,
	pub blobs_url: String,
	pub branches_url: String,
	pub collaborators_url: String,
	pub comments_url: String,
	pub commits_url: String,
	pub compare_url: String,
	pub contents_url: String,
	pub contributors_url: String,
	pub deployments_url: String,
	pub downloads_url: String,
	pub events_url: String,
	pub forks_url: String,
	pub git_commits_url: String,
	pub git_refs_url: String,
	pub git_tags_url: String,
	pub git_url: String,
	pub issue_comment_url: String,
	pub issue_events_url: String,
	pub issues_url: String,
	pub keys_url: String,
	pub labels_url: Option<String>,
	pub languages_url: String,
	pub merges_url: String,
	pub milestones_url: String,
	pub notifications_url: String,
	pub pulls_url: String,
	pub releases_url: String,
	pub ssh_url: String,
	pub stargazers_url: String,
	pub statuses_url: String,
	pub subscribers_url: String,
	pub subscription_url: String,
	pub tags_url: String,
	pub teams_url: String,
	pub trees_url: String,
	pub clone_url: String,
	pub mirror_url: Option<String>,
	pub hooks_url: String,
	pub svn_url: String,
	pub homepage: Option<String>,
	pub language: Option<::serde_json::Value>,
	pub forks_count: i64,
	pub stargazers_count: i64,
	pub watchers_count: i64,
	pub size: i64,
	pub default_branch: String,
	pub open_issues_count: i64,
	pub is_template: Option<bool>,
	pub topics: Option<Vec<String>>,
	pub has_issues: bool,
	pub has_projects: bool,
	pub has_wiki: bool,
	pub has_pages: bool,
	pub has_downloads: bool,
	pub archived: bool,
	pub disabled: bool,
	pub visibility: Option<String>,
	pub pushed_at: String,
	pub created_at: String,
	pub updated_at: String,
	pub permissions: Option<Permissions>,
	pub allow_rebase_merge: Option<bool>,
	pub template_repository: Option<::serde_json::Value>,
	pub allow_squash_merge: Option<bool>,
	pub allow_merge_commit: Option<bool>,
	pub subscribers_count: Option<i64>,
	pub network_count: Option<i64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
	admin: Option<bool>,
	push: Option<bool>,
	pull: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombinedStatus {
	pub state: StatusState,
	pub sha: String,
	pub total_count: i64,
	pub statuses: Vec<Status>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
	pub id: Option<i64>,
	pub node_id: Option<String>,
	pub avatar_url: Option<String>,
	pub url: Option<String>,
	pub created_at: Option<chrono::DateTime<chrono::Utc>>,
	pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
	pub state: StatusState,
	pub creator: Option<User>,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
	Failure,
	Pending,
	Success,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
	Approved,
	Pending,
	ChangesRequested,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Links {
	#[serde(rename = "self")]
	pub self_link: Option<SelfLink>,
	pub html_link: Option<HtmlLink>,
	pub issue_link: Option<IssueLink>,
	pub comments_link: Option<CommentsLink>,
	pub review_comments_link: Option<ReviewCommentsLink>,
	pub review_comment_link: Option<ReviewCommentLink>,
	pub commits_link: Option<CommitsLink>,
	pub statuses_link: Option<StatusesLink>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HtmlLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommentsLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewCommentsLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewCommentLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitsLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusesLink {
	pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
	pub id: i64,
	pub account: User,
	pub access_tokens_url: Option<String>,
	pub repositories_url: Option<String>,
	pub html_url: Option<String>,
	pub app_id: Option<i64>,
	pub target_id: Option<i64>,
	pub target_type: Option<String>,
	pub permissions: InstallationPermissions,
	pub events: Vec<String>,
	pub single_file_name: Option<String>,
	pub repository_selection: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationPermissions {
	pub metadata: String,
	pub contents: String,
	pub issues: String,
	pub single_file: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationToken {
	pub token: String,
	pub expires_at: Option<String>,
	pub permissions: Permissions,
	pub repositories: Option<Vec<Repository>>,
}
