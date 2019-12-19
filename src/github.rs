use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Organisation {
	pub login: String,
	pub id: i64,
	pub node_id: String,
	pub url: String,
	pub repos_url: String,
	pub events_url: String,
	pub hooks_url: String,
	pub issues_url: String,
	pub members_url: String,
	pub public_members_url: String,
	pub avatar_url: String,
	pub description: Option<String>,
	pub name: Option<String>,
	pub company: Option<String>,
	pub blog: Option<String>,
	pub location: Option<String>,
	pub email: Option<String>,
	pub is_verified: bool,
	pub has_organization_projects: bool,
	pub has_repository_projects: bool,
	pub public_repos: i64,
	pub public_gists: i64,
	pub followers: i64,
	pub following: i64,
	pub html_url: String,
	pub created_at: String,
	#[serde(rename = "type")]
	pub type_field: String,
	pub total_private_repos: i64,
	pub owned_private_repos: i64,
	pub private_gists: i64,
	pub disk_usage: i64,
	pub collaborators: i64,
	pub billing_email: Option<String>,
	pub plan: Option<Plan>,
	pub default_repository_settings: Option<String>,
	pub members_can_create_repositories: Option<bool>,
	pub two_factor_requirement_enabled: bool,
	pub members_allowed_repository_creation_type: Option<String>,
	pub members_can_create_public_repositories: Option<bool>,
	pub members_can_create_private_repositories: Option<bool>,
	pub members_can_create_internal_repositories: Option<bool>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
	pub name: String,
	pub space: i64,
	pub private_repos: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
	pub url: String,
	pub id: i64,
	pub node_id: String,
	pub html_url: String,
	pub diff_url: String,
	pub patch_url: String,
	pub issue_url: String,
	pub commits_url: String,
	pub review_comments_url: String,
	pub review_comment_url: String,
	pub comments_url: String,
	pub statuses_url: String,
	pub number: i64,
	pub state: String,
	pub locked: bool,
	pub title: String,
	pub user: User,
	pub body: Option<String>,
	pub labels: Vec<Label>,
	pub milestone: Option<Milestone>,
	pub active_lock_reason: Option<String>,
	pub created_at: String,
	pub updated_at: String,
	pub closed_at: Option<String>,
	pub merged_at: Option<String>,
	pub merge_commit_sha: Option<String>,
	pub assignee: Option<User>,
	pub assignees: Vec<User>,
	pub requested_reviewers: Vec<User>,
	pub requested_teams: Vec<RequestedTeam>,
	pub head: Head,
	pub base: Base,
	#[serde(rename = "_links")]
	pub links: Links,
	pub author_association: Option<String>,
	pub draft: Option<bool>,
	#[serde(rename = "repo")]
        pub repository: Repository,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
	pub id: i64,
	pub node_id: String,
	pub html_url: String,
	pub user: User,
	pub body: Option<String>,
        pub commit_id: String,
	pub state: String,
        pub pull_request_url: String,
	#[serde(rename = "_links")]
        pub links: Links,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
	pub id: i64,
	pub node_id: String,
	pub html_url: String,
	pub user: User,
	pub body: Option<String>,
	pub title: String,
	pub state: String,
	pub labels: Vec<Label>,
	pub assignee: Option<User>,
	pub assignees: Vec<User>,
	pub milestone: Option<Milestone>,
	pub locked: bool,
	pub active_lock_reason: Option<String>,
        pub pull_request: Option<PullRequest>,
	pub created_at: String,
	pub updated_at: String,
	pub closed_at: Option<String>,
        pub repository: Repository,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
	pub login: String,
	pub id: i64,
	pub node_id: String,
	pub avatar_url: String,
	pub gravatar_id: String,
	pub url: String,
	pub html_url: String,
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
	id: i64,
	node_id: String,
	url: String,
	name: String,
	description: Option<String>,
	color: String,
	default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Milestone {
	url: String,
	html_url: String,
	labels_url: String,
	id: i64,
	node_id: String,
	number: i64,
	state: String,
	title: String,
	description: Option<String>,
	creator: User,
	open_issues: i64,
	closed_issues: i64,
	created_at: String,
	updated_at: String,
	closed_at: String,
	due_on: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestedTeam {
	id: i64,
	node_id: String,
	url: String,
	html_url: String,
	name: String,
	slug: String,
	description: Option<String>,
	privacy: String,
	permission: String,
	members_url: String,
	repositories_url: String,
	parent: ::serde_json::Value,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Head {
	label: String,
	#[serde(rename = "ref")]
	ref_field: String,
	sha: String,
	user: User,
	repo: Repository,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repository {
	pub id: i64,
	pub node_id: String,
	pub name: String,
	pub full_name: String,
	pub owner: User,
	pub private: bool,
	pub html_url: String,
	pub description: Option<String>,
	pub fork: bool,
	pub url: String,
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
	pub labels_url: String,
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
	admin: bool,
	push: bool,
	pull: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
	label: String,
	#[serde(rename = "ref")]
	ref_field: String,
	sha: String,
	user: User,
	repo: Repository,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
	pub id: i64,
	pub node_id: String,
	pub avatar_url: String,
	pub url: String,
	pub created_at: String,
	pub updated_at: String,
	pub state: String,
	pub creator: User,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Links {
	#[serde(rename = "self")]
	pub self_link: SelfLink,
	pub html_link: HtmlLink,
	pub issue_link: IssueLink,
	pub comments_link: CommentsLink,
	pub review_comments_link: ReviewCommentsLink,
	pub review_comment_link: ReviewCommentLink,
	pub commits_link: CommitsLink,
	pub statuses_link: StatusesLink,
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
