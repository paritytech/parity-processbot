use crate::{
	constants::*, error::*, github::GithubBot, types::*, PR_HTML_URL_REGEX,
};
use regex::Regex;
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use snafu::OptionExt;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
	pub url: String,
	pub html_url: String,
	pub number: usize,
	pub user: Option<User>,
	pub body: Option<String>,
	pub labels: Vec<Label>,
	pub mergeable: Option<bool>,
	pub head: Option<Head>,
	pub base: Base,
	pub repository: Option<Repository>,
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
	pub number: usize,
	pub html_url: String,
	// User might be missing when it has been deleted
	pub user: Option<User>,
	pub body: Option<String>,
	pub pull_request: Option<IssuePullRequest>,
	pub repository: Option<Repository>,
	pub repository_url: Option<String>,
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
	pub id: Option<usize>,
	pub project_id: Option<usize>,
	pub project_url: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
	pub id: usize,
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
	pub id: usize,
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
	Bot,
	#[serde(other)]
	Unknown,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
	pub id: usize,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombinedStatus {
	pub statuses: Vec<Status>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
	pub id: usize,
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
	pub id: usize,
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
	pub total_count: usize,
	pub check_runs: Vec<CheckRun>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadRepo {
	pub id: usize,
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
	pub id: usize,
	pub name: String,
	pub status: CheckRunStatus,
	pub conclusion: Option<CheckRunConclusion>,
	pub head_sha: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookIssueComment {
	pub number: usize,
	pub html_url: String,
	pub repository_url: Option<String>,
	pub pull_request: Option<IssuePullRequest>,
}

impl HasIssueDetails for WebhookIssueComment {
	fn get_issue_details(&self) -> Option<IssueDetails> {
		None
	}
}

#[derive(PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Payload {
	IssueComment {
		action: IssueCommentAction,
		issue: WebhookIssueComment,
		comment: Comment,
	},
	CommitStatus {
		// FIXME: This payload also has a field `repository` for the repository where the status
		// originated from which should be used *together* with commit SHA for indexing pull requests.
		// Currently, because merge requests are indexed purely by their head SHA into the database,
		// there's no way to disambiguate between two different PRs in two different repositories with
		// the same head SHA.
		sha: String,
		state: StatusState,
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
struct DetectUserCommentPullRequestIssue {
	pub pull_request: Option<DetectUserCommentPullRequestPullRequest>,
	pub number: usize,
}

#[derive(Deserialize)]
struct DetectUserCommentPullRequestComment {
	pub body: Option<String>,
}

#[derive(Deserialize)]
pub struct DetectUserCommentPullRequest {
	action: IssueCommentAction,
	issue: Option<DetectUserCommentPullRequestIssue>,
	repository: Option<Repository>,
	sender: Option<User>,
	comment: Option<DetectUserCommentPullRequestComment>,
}

impl HasIssueDetails for DetectUserCommentPullRequest {
	fn get_issue_details(&self) -> Option<IssueDetails> {
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
			..
		} = self
		{
			match self.sender {
				Some(User {
					type_field: Some(UserType::Bot),
					..
				}) => None,
				_ => {
					let body = body.trim();

					if BOT_COMMANDS.iter().find(|cmd| **cmd == body).is_some() {
						if let Some(Repository {
							name: name,
							owner: Some(User { login, .. }),
							..
						}) = repository
						{
							Some(IssueDetails {
								owner: login.to_owned(),
								repo: name.to_owned(),
								number: *number,
							})
						} else if let Some(html_url) = pr.html_url.as_ref() {
							get_issue_details_fallback(
								repository.as_ref(),
								&html_url,
								*number,
							)
						} else {
							None
						}
					} else {
						None
					}
				}
			}
		} else {
			None
		}
	}
}

pub trait HasIssueDetails {
	fn get_issue_details(&self) -> Option<IssueDetails>;
}

fn get_issue_details_fallback(
	repo: Option<&Repository>,
	html_url: &str,
	number: usize,
) -> Option<IssueDetails> {
	if let Some(Repository {
		full_name: Some(full_name),
		..
	}) = repo.as_ref()
	{
		parse_repository_full_name(full_name).map(|(owner, name)| {
			IssueDetails {
				owner,
				repo: name,
				number,
			}
		})
	} else {
		parse_issue_details_from_pr_html_url(html_url)
	}
}

impl HasIssueDetails for PullRequest {
	fn get_issue_details(&self) -> Option<IssueDetails> {
		let repo = self.repository.as_ref();
		if let Some(Repository {
			owner: Some(User { login, .. }),
			name,
			..
		}) = repo
		{
			Some(IssueDetails {
				owner: login.to_owned(),
				repo: name.to_owned(),
				number: self.number,
			})
		} else {
			get_issue_details_fallback(repo, &self.html_url, self.number)
		}
	}
}

impl HasIssueDetails for Issue {
	fn get_issue_details(&self) -> Option<IssueDetails> {
		match self {
			Issue {
				pull_request: Some(_), // indicates the issue is a pr
				repository,
				..
			} => {
				if let Some(Repository {
					owner: Some(User { login, .. }),
					name,
					..
				}) = &repository
				{
					Some(IssueDetails {
						owner: login.to_owned(),
						repo: name.to_owned(),
						number: self.number,
					})
				} else {
					get_issue_details_fallback(
						repository.as_ref(),
						&self.html_url,
						self.number,
					)
				}
			}
		}
	}
}

#[derive(Debug)]
pub struct IssueDetails {
	owner: String,
	repo: String,
	number: usize,
}

#[derive(Debug)]
pub struct IssueDetailsWithRepositoryURL {
	issue: IssueDetails,
	repo_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	owner: String,
	repo_name: String,
	number: usize,
	html_url: String,
	requested_by: String,
	head_sha: String,
}

pub enum Status {
	Success,
	Pending,
	Failure,
}

pub struct AppState {
	pub db: DB,
	pub github_bot: GithubBot,
	pub webhook_secret: String,
}

impl AppState {
	pub async fn check_statuses(&self, commit_sha: &str) -> Result<()> {
		self.github_bot.check_statuses(self.db, commit_sha)
	}
}
