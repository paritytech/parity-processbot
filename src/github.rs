use crate::{constants::BOT_COMMANDS, error::*, Result, PR_HTML_URL_REGEX};
use regex::Regex;
use serde::{Deserialize, Serialize};
use snafu::OptionExt;

pub trait HasIssueDetails {
	fn get_issue_details(&self) -> Option<IssueDetails>;
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
	pub url: String,
	pub id: i64,
	pub node_id: Option<String>,
	pub html_url: String,
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
	pub user: Option<User>,
	pub body: Option<String>,
	pub labels: Vec<Label>,
	pub milestone: Option<Milestone>,
	pub active_lock_reason: Option<String>,
	pub created_at: Option<chrono::DateTime<chrono::Utc>>,
	pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
	pub closed_at: Option<String>,
	pub merged: Option<bool>,
	pub mergeable: Option<bool>,
	pub merged_at: Option<String>,
	pub merge_commit_sha: Option<String>,
	pub assignee: Option<User>,
	pub assignees: Option<Vec<User>>,
	pub requested_reviewers: Option<Vec<User>>,
	pub requested_teams: Option<Vec<RequestedTeam>>,
	// Head might be missing when e.g. the branch has been deleted
	pub head: Option<Head>,
	pub base: Base,
	#[serde(rename = "_links")]
	pub links: Option<Links>,
	pub author_association: Option<String>,
	pub draft: Option<bool>,
	#[serde(rename = "repo")]
	pub repository: Option<Repository>,
}

impl HasIssueDetails for PullRequest {
	fn get_issue_details(&self) -> Option<(String, String, i64)> {
		if let Some(Repository {
			owner: Some(User { login, .. }),
			name,
			..
		}) = self.repository.as_ref()
		{
			Some((login.to_owned(), name.to_owned(), self.number))
		} else {
			None
		}
		.or_else(|| {
			if let Some(Repository {
				full_name: Some(full_name),
				..
			}) = self.repository.as_ref()
			{
				parse_repository_full_name(full_name)
					.map(|(owner, name)| (owner, name, self.number))
			} else {
				None
			}
		})
		.or_else(|| parse_issue_details_from_pr_html_url(&self.html_url))
	}
}

impl PullRequest {
	pub fn head_sha(&self) -> Result<&String> {
		self.head
			.as_ref()
			.context(MissingField {
				field: "pull_request.head",
			})?
			.sha
			.as_ref()
			.context(MissingField {
				field: "pull_request.head.sha",
			})
	}
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
	pub number: i64,
	pub id: i64,
	pub node_id: Option<String>,
	pub html_url: String,
	// User might be missing when it has been deleted
	pub user: Option<User>,
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
	pub repository_url: Option<String>,
}

impl HasIssueDetails for Issue {
	fn get_issue_details(&self) -> Option<(String, String, i64)> {
		match self {
			Issue {
				number,
				html_url,
				pull_request: Some(_), // indicates the issue is a pr
				repository,
				..
			} => if let Some(Repository {
				owner: Some(User { login, .. }),
				name,
				..
			}) = &repository
			{
				Some((login.to_owned(), name.to_owned(), *number))
			} else {
				None
			}
			.or_else(|| {
				if let Some(Repository {
					full_name: Some(full_name),
					..
				}) = &repository
				{
					parse_repository_full_name(full_name)
						.map(|(owner, name)| (owner, name, *number))
				} else {
					None
				}
			})
			.or_else(|| parse_issue_details_from_pr_html_url(&html_url)),
			_ => None,
		}
	}
}

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
	RemovedFromProject,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueEvent {
	pub project_card: Option<ProjectCard>,
	pub event: Option<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comment {
	pub id: i64,
	pub body: String,
	// User might be missing when it has been deleted
	pub user: Option<User>,
	pub node_id: Option<String>,
	pub url: Option<String>,
	pub html_url: Option<String>,
	pub created_at: chrono::DateTime<chrono::Utc>,
	pub updated_at: chrono::DateTime<chrono::Utc>,
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
	pub id: i64,
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
pub struct Head {
	pub label: Option<String>,
	#[serde(rename = "ref")]
	pub ref_field: Option<String>,
	pub sha: Option<String>,
	// Repository might be missing when it has been deleted
	pub repo: Option<HeadRepo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
	#[serde(rename = "ref")]
	pub ref_field: Option<String>,
	pub sha: Option<String>,
	// Repository might be missing when it has been deleted
	pub repo: Option<HeadRepo>,
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

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum UserType {
	User,
	#[serde(other)]
	Unknown,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
	pub login: String,
	#[serde(rename = "type")]
	pub type_field: Option<UserType>,
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
	pub id: i64,
	pub node_id: Option<String>,
	pub name: String,
	pub full_name: Option<String>,
	pub owner: Option<User>,
	pub private: Option<bool>,
	pub html_url: String,
	pub description: Option<String>,
	pub fork: Option<bool>,
	pub url: Option<String>,
	pub archive_url: Option<String>,
	pub assignees_url: Option<String>,
	pub blobs_url: Option<String>,
	pub branches_url: Option<String>,
	pub collaborators_url: Option<String>,
	pub comments_url: Option<String>,
	pub commits_url: Option<String>,
	pub compare_url: Option<String>,
	pub contents_url: Option<String>,
	pub contributors_url: Option<String>,
	pub deployments_url: Option<String>,
	pub downloads_url: Option<String>,
	pub events_url: Option<String>,
	pub forks_url: Option<String>,
	pub git_commits_url: Option<String>,
	pub git_refs_url: Option<String>,
	pub git_tags_url: Option<String>,
	pub git_url: Option<String>,
	pub issue_comment_url: Option<String>,
	pub issue_events_url: Option<String>,
	pub issues_url: Option<String>,
	pub keys_url: Option<String>,
	pub labels_url: Option<String>,
	pub languages_url: Option<String>,
	pub merges_url: Option<String>,
	pub milestones_url: Option<String>,
	pub notifications_url: Option<String>,
	pub pulls_url: Option<String>,
	pub releases_url: Option<String>,
	pub ssh_url: Option<String>,
	pub stargazers_url: Option<String>,
	pub statuses_url: Option<String>,
	pub subscribers_url: Option<String>,
	pub subscription_url: Option<String>,
	pub tags_url: Option<String>,
	pub teams_url: Option<String>,
	pub trees_url: Option<String>,
	pub clone_url: Option<String>,
	pub mirror_url: Option<String>,
	pub hooks_url: Option<String>,
	pub svn_url: Option<String>,
	pub homepage: Option<String>,
	pub language: Option<::serde_json::Value>,
	pub forks_count: Option<i64>,
	pub stargazers_count: Option<i64>,
	pub watchers_count: Option<i64>,
	pub size: Option<i64>,
	pub default_branch: Option<String>,
	pub open_issues_count: Option<i64>,
	pub is_template: Option<bool>,
	pub topics: Option<Vec<String>>,
	pub has_issues: Option<bool>,
	pub has_projects: Option<bool>,
	pub has_wiki: Option<bool>,
	pub has_pages: Option<bool>,
	pub has_downloads: Option<bool>,
	pub archived: Option<bool>,
	pub disabled: Option<bool>,
	pub visibility: Option<String>,
	pub pushed_at: Option<String>,
	pub created_at: Option<String>,
	pub updated_at: Option<String>,
	pub permissions: Option<Permissions>,
	pub allow_rebase_merge: Option<bool>,
	pub template_repository: Option<::serde_json::Value>,
	pub allow_squash_merge: Option<bool>,
	pub allow_merge_commit: Option<bool>,
	pub subscribers_count: Option<i64>,
	pub network_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
	pub url: Option<String>,
	pub sha: Option<String>,
	pub node_id: Option<String>,
	pub html_url: Option<String>,
	pub comments_url: Option<String>,
	pub author: User,
	pub committer: User,
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
	pub context: String,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
	Error,
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
	Commented,
	Dismissed,
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
pub struct InstallationRepositories {
	pub total_count: i64,
	pub repositories: Vec<Repository>,
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
	pub url: String,
	pub html_url: String,
	pub tarball_url: String,
	pub zipball_url: String,
	pub id: i64,
	pub tag_name: String,
	pub target_commitish: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ref {
	pub object: RefObject,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefObject {
	#[serde(rename = "type")]
	pub ref_type: String,
	pub sha: String,
	pub url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diff {
	pub url: String,
	pub html_url: String,
	pub permalink_url: Option<String>,
	pub diff_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCommentAction {
	Created,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckRunAction {
	Created,
	Completed,
	Rerequested,
	RequestedAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, rename_all = "snake_case")]
pub enum CheckRunStatus {
	Queued,
	InProgress,
	Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, rename_all = "snake_case")]
pub enum CheckRunConclusion {
	Success,
	Failure,
	Neutral,
	Cancelled,
	TimedOut,
	ActionRequired,
	Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CheckRuns {
	pub total_count: i64,
	pub check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CheckRunPR {
	pub id: i64,
	pub number: i64,
	pub head: Head,
	pub base: Base,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadRepo {
	pub id: i64,
	pub url: String,
	pub name: String,
	// The owner might be missing when e.g. they have deleted their account
	pub owner: Option<User>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CheckRun {
	pub status: String,
	pub conclusion: Option<String>,
	pub head_sha: String,
	pub pull_requests: Vec<CheckRunPR>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BranchCommit {
	pub sha: String,
	pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Branch {
	pub name: String,
	pub commit: BranchCommit,
	pub protected: bool,
}

#[derive(PartialEq, Deserialize)]
#[serde(untagged, rename_all = "snake_case")]
pub enum Payload {
	IssueComment {
		action: IssueCommentAction,
		issue: Issue,
		comment: Comment,
	},
	CommitStatus {
		sha: String,
		state: StatusState,
		description: String,
		target_url: String,
		repository: Repository,
		branches: Vec<Branch>,
	},
	CheckRun {
		action: CheckRunAction,
		check_run: CheckRun,
	},
}

#[derive(Deserialize)]
struct DetectUserCommentPullRequestPullRequest {
	pub html_url: Option<String>,
}

#[derive(Deserialize)]
struct DetectUserCommentPullRequestRepository {
	pub name: Option<String>,
	pub full_name: Option<String>,
	pub owner: Option<User>,
}

#[derive(Deserialize)]
struct DetectUserCommentPullRequestIssue {
	pub pull_request: Option<DetectUserCommentPullRequestPullRequest>,
	pub number: i64,
}

#[derive(Deserialize)]
struct DetectUserCommentPullRequestComment {
	pub body: Option<String>,
}

#[derive(Deserialize)]
pub struct DetectUserCommentPullRequest {
	action: IssueCommentAction,
	issue: Option<DetectUserCommentPullRequestIssue>,
	repository: Option<DetectUserCommentPullRequestRepository>,
	sender: Option<User>,
	comment: Option<DetectUserCommentPullRequestComment>,
}

impl HasIssueDetails for DetectUserCommentPullRequest {
	fn get_issue_details(&self) -> Option<(String, String, i64)> {
		if let DetectUserCommentPullRequest {
			action: IssueCommentAction::Created,
			issue:
				Some(DetectUserCommentPullRequestIssue {
					number,
					pull_request: Some(pr),
				}),
			comment:
				Some(DetectUserCommentPullRequestComment { body: Some(body) }),
			repository,
			sender:
				Some(User {
					type_field: Some(UserType::User),
					..
				}),
		} = self
		{
			let body = body.trim();

			if BOT_COMMANDS.iter().find(|cmd| **cmd == body).is_some() {
				if let Some(DetectUserCommentPullRequestRepository {
					name: Some(name),
					owner: Some(User { login, .. }),
					..
				}) = repository
				{
					Some((login.to_owned(), name.to_owned(), *number))
				} else {
					None
				}
				.or_else(|| {
					if let Some(DetectUserCommentPullRequestRepository {
						full_name: Some(full_name),
						..
					}) = repository
					{
						parse_repository_full_name(full_name)
							.map(|(owner, name)| (owner, name, *number))
					} else {
						None
					}
				})
				.or_else(|| {
					if let DetectUserCommentPullRequestPullRequest {
						html_url: Some(html_url),
					} = pr
					{
						parse_issue_details_from_pr_html_url(html_url)
							.map(|(owner, name, _)| (owner, name, *number))
					} else {
						None
					}
				})
			} else {
				None
			}
		} else {
			None
		}
	}
}

pub fn parse_issue_details_from_pr_html_url(
	pr_html_url: &str,
) -> Option<(String, String, i64)> {
	let re = Regex::new(PR_HTML_URL_REGEX!()).unwrap();
	let matches = re.captures(&pr_html_url)?;
	let owner = matches.name("owner")?.as_str().to_owned();
	let repo = matches.name("repo")?.as_str().to_owned();
	let number = matches
		.name("number")?
		.as_str()
		.to_owned()
		.parse::<i64>()
		.ok()?;
	Some((owner, repo, number))
}

pub fn parse_repository_full_name(full_name: &str) -> Option<(String, String)> {
	let parts: Vec<&str> = full_name.split("/").collect();
	parts
		.get(0)
		.map(|owner| {
			parts.get(1).map(|repo_name| {
				Some((owner.to_string(), repo_name.to_string()))
			})
		})
		.flatten()
		.flatten()
}
