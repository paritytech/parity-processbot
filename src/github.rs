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
	pub html_url: String,
	pub number: i64,
	pub user: Option<User>,
	pub body: Option<String>,
	pub labels: Vec<Label>,
	pub mergeable: Option<bool>,
	pub head: Option<Head>,
	pub base: Base,
	pub repository: Option<Repository>,
}

#[derive(Serialize, Deserialize)]
pub struct TreeObject<'a> {
	pub path: &'a str,
	pub content: String,
	// file mode in the Linux format as a string e.g. "100644"
	pub mode: String,
}

#[derive(Deserialize)]
pub struct CreatedTree {
	pub sha: String,
}

#[derive(Deserialize)]
pub struct CreatedCommit {
	pub sha: String,
}

#[derive(Deserialize)]
pub struct CreatedRef {
	#[serde(rename = "ref")]
	pub ref_field: Option<String>,
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
				parse_repository_full_name(&full_name)
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
	pub html_url: String,
	// User might be missing when it has been deleted
	pub user: Option<User>,
	pub body: Option<String>,
	pub pull_request: Option<IssuePullRequest>,
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
pub struct Contents {
	pub content: String,
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
	pub body: String,
	// User might be missing when it has been deleted
	pub user: Option<User>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCard {
	pub id: Option<i64>,
	pub project_id: Option<i64>,
	pub project_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
	pub id: Option<i64>,
	pub name: String,
	pub columns_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProjectCardContentType {
	Issue,
	PullRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectColumn {
	pub name: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
	pub id: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssuePullRequest {}

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
	pub id: i64,
	// User might be missing when it has been deleted
	pub user: Option<User>,
	pub state: Option<ReviewState>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestedReviewers {
	pub users: Vec<User>,
	pub teams: Vec<Team>,
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum UserType {
	User,
	Bot,
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
	pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repository {
	pub name: String,
	pub full_name: Option<String>,
	pub owner: Option<User>,
	pub html_url: String,
	pub issues_url: Option<String>,
	pub pulls_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombinedStatus {
	pub statuses: Vec<Status>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
	pub id: i64,
	pub context: String,
	pub state: StatusState,
	pub description: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
	Pending,
	Success,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
	Approved,
	ChangesRequested,
	#[serde(other)]
	Unknown,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallationRepositories {
	pub repositories: Vec<Repository>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Installation {
	pub id: i64,
	pub account: User,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallationToken {
	pub token: String,
	pub expires_at: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
	pub tag_name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ref {
	pub object: RefObject,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefObject {
	pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCommentAction {
	Created,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRuns {
	pub total_count: i64,
	pub check_runs: Vec<CheckRun>,
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
pub enum CheckRunConclusion {
	Success,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckRunStatus {
	Completed,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRun {
	pub id: i64,
	pub name: String,
	pub status: CheckRunStatus,
	pub conclusion: Option<CheckRunConclusion>,
	pub head_sha: String,
}

#[derive(PartialEq, Deserialize)]
#[serde(untagged)]
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
	},
	CheckRun {
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
