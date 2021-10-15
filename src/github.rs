use crate::{
	companion::parse_all_companions, error::*,
	utils::parse_bot_comment_from_text, webhook::MergeRequestBase,
	PlaceholderDeserializationItem, PR_HTML_URL_REGEX,
};
use regex::Regex;
use serde::{Deserialize, Serialize};

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
	pub head: Head,
	pub base: Base,
	pub repository: Option<Repository>,
	pub mergeable: Option<bool>,
	pub merged: bool,
	pub maintainer_can_modify: bool,
}

impl PullRequest {
	pub fn parse_all_companions(
		&self,
		companion_reference_trail: &Vec<(String, String)>,
	) -> Option<Vec<IssueDetailsWithRepositoryURL>> {
		let mut next_trail: Vec<(String, String)> =
			Vec::with_capacity(companion_reference_trail.len() + 1);
		next_trail.extend_from_slice(&companion_reference_trail[..]);
		next_trail.push((
			self.base.repo.owner.login.to_owned(),
			self.base.repo.name.to_owned(),
		));
		self.body
			.as_ref()
			.map(|body| parse_all_companions(&next_trail, body))
	}

	pub fn parse_all_mr_base(
		&self,
		companion_reference_trail: &Vec<(String, String)>,
	) -> Option<Vec<MergeRequestBase>> {
		self.parse_all_companions(companion_reference_trail)
			.map(|companions| {
				companions
					.into_iter()
					.map(|(_, owner, repo, number)| MergeRequestBase {
						owner,
						repo,
						number,
					})
					.collect()
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
	pub pull_request: Option<PlaceholderDeserializationItem>,
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
pub struct Head {
	pub sha: String,
	pub repo: HeadRepo,
	#[serde(rename = "ref")]
	pub ref_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
	#[serde(rename = "ref")]
	pub ref_field: String,
	pub repo: BaseRepo,
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
	pub owner: User,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseRepo {
	pub name: String,
	pub owner: User,
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

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookIssueComment {
	pub number: i64,
	pub html_url: String,
	pub repository_url: Option<String>,
	pub pull_request: Option<PlaceholderDeserializationItem>,
}

impl HasIssueDetails for WebhookIssueComment {
	fn get_issue_details(&self) -> Option<IssueDetails> {
		parse_issue_details_from_pr_html_url(&self.html_url)
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowJobConclusion {
	#[serde(other)]
	Unknown,
}
#[derive(PartialEq, Deserialize)]
pub struct WorkflowJob {
	pub head_sha: String,
	pub conclusion: Option<WorkflowJobConclusion>,
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
	WorkflowJob {
		workflow_job: WorkflowJob,
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
					if parse_bot_comment_from_text(body).is_none() {
						return None;
					}

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
				}
			}
		} else {
			None
		}
	}
}

pub fn parse_issue_details_from_pr_html_url(
	pr_html_url: &str,
) -> Option<IssueDetails> {
	let re = Regex::new(PR_HTML_URL_REGEX!()).unwrap();
	let matches = re.captures(pr_html_url)?;
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
	let parts: Vec<&str> = full_name.split('/').collect();
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
