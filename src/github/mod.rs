use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
	bot::parse_bot_comment_from_text,
	companion::{parse_all_companions, CompanionReferenceTrailItem},
	error::*,
	types::PlaceholderDeserializationItem,
	OWNER_AND_REPO_SEQUENCE, PR_HTML_URL_REGEX,
};

mod client;

pub trait HasPullRequestDetails {
	fn get_pull_request_details(&self) -> Option<PullRequestDetails>;
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubPullRequest {
	pub url: String,
	pub html_url: String,
	pub number: i64,
	pub user: Option<GithubUser>,
	pub body: Option<String>,
	pub head: GithubPullRequestHead,
	pub base: GithubPullRequestBase,
	pub mergeable: Option<bool>,
	pub merged: bool,
	pub maintainer_can_modify: bool,
}

impl GithubPullRequest {
	pub fn parse_all_companions(
		&self,
		companion_reference_trail: &[CompanionReferenceTrailItem],
	) -> Option<Vec<PullRequestDetailsWithHtmlUrl>> {
		let mut next_trail =
			Vec::with_capacity(companion_reference_trail.len() + 1);
		next_trail.extend_from_slice(companion_reference_trail);
		next_trail.push(CompanionReferenceTrailItem {
			owner: (&self.base.repo.owner.login).into(),
			repo: (&self.base.repo.name).into(),
		});
		self.body
			.as_ref()
			.map(|body| parse_all_companions(&next_trail, body))
	}
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubFileContents {
	pub content: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubPullRequestBase {
	#[serde(rename = "ref")]
	pub ref_field: String,
	pub repo: GithubPullRequestBaseRepository,
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum GithubUserType {
	User,
	Bot,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct GithubUser {
	pub login: String,
	#[serde(rename = "type")]
	pub type_field: GithubUserType,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubRepository {
	pub name: String,
	pub full_name: String,
	pub owner: GithubUser,
	pub html_url: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubCommitStatus {
	pub id: i64,
	pub context: String,
	pub state: GithubCommitStatusState,
	pub description: Option<String>,
	pub target_url: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubCommitStatusState {
	Success,
	Error,
	Failure,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubInstallation {
	pub id: i64,
	pub account: GithubUser,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubInstallationToken {
	pub token: String,
	pub expires_at: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubIssueCommentAction {
	Created,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubCheckRuns {
	pub check_runs: Vec<GithubCheckRun>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubPullRequestHeadRepository {
	pub name: String,
	pub owner: GithubUser,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubPullRequestHead {
	pub sha: String,
	pub repo: GithubPullRequestHeadRepository,
	#[serde(rename = "ref")]
	pub ref_field: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubPullRequestBaseRepository {
	pub name: String,
	pub owner: GithubUser,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubCheckRunConclusion {
	Success,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubCheckRunStatus {
	Completed,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubCheckRun {
	pub id: i64,
	pub name: String,
	pub status: GithubCheckRunStatus,
	pub conclusion: Option<GithubCheckRunConclusion>,
	pub head_sha: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GithubIssue {
	pub number: i64,
	pub html_url: String,
	pub pull_request: Option<PlaceholderDeserializationItem>,
}
impl HasPullRequestDetails for GithubIssue {
	fn get_pull_request_details(&self) -> Option<PullRequestDetails> {
		parse_pull_request_details_from_url(&self.html_url)
	}
}

#[derive(PartialEq, Eq, Deserialize)]
pub struct GithubIssueComment {
	pub id: i64,
	pub body: String,
	pub user: GithubUser,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubWorkflowJobConclusion {
	#[serde(other)]
	Unknown,
}

#[derive(PartialEq, Eq, Deserialize)]
pub struct GithubWorkflowJob {
	pub head_sha: String,
	pub conclusion: Option<GithubWorkflowJobConclusion>,
}

#[derive(Deserialize, PartialEq, Eq)]
pub struct GithubIssueRepository {
	pub owner: GithubUser,
	pub name: String,
}

#[derive(PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum GithubWebhookPayload {
	IssueComment {
		action: GithubIssueCommentAction,
		issue: GithubIssue,
		comment: GithubIssueComment,
		repository: GithubIssueRepository,
	},
	CommitStatus {
		// FIXME: This payload also has a field `repository` for the repository where the status
		// originated from which should be used *together* with commit SHA for indexing pull requests.
		// Currently, because merge requests are indexed purely by their head SHA into the database,
		// there's no way to disambiguate between two different PRs in two different repositories with
		// the same head SHA.
		sha: String,
		state: GithubCommitStatusState,
	},
	CheckRun {
		check_run: GithubCheckRun,
	},
	WorkflowJob {
		workflow_job: GithubWorkflowJob,
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
	pub owner: Option<GithubUser>,
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
	action: GithubIssueCommentAction,
	issue: Option<DetectUserCommentPullRequestIssue>,
	repository: Option<DetectUserCommentPullRequestRepository>,
	sender: Option<GithubUser>,
	comment: Option<DetectUserCommentPullRequestComment>,
}

impl HasPullRequestDetails for DetectUserCommentPullRequest {
	fn get_pull_request_details(&self) -> Option<PullRequestDetails> {
		if let DetectUserCommentPullRequest {
			action: GithubIssueCommentAction::Created,
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
				Some(GithubUser {
					type_field: GithubUserType::Bot,
					..
				}) => None,
				_ => {
					parse_bot_comment_from_text(body)?;

					if let Some(DetectUserCommentPullRequestRepository {
						name: Some(name),
						owner: Some(GithubUser { login, .. }),
						..
					}) = repository
					{
						Some(PullRequestDetails {
							owner: login.into(),
							repo: name.into(),
							number: *number,
						})
					} else {
						None
					}
					.or_else(|| {
						if let Some(DetectUserCommentPullRequestRepository {
							full_name: Some(full_name),
							..
						}) = repository
						{
							parse_repository_full_name(full_name).map(
								|(owner, repo)| PullRequestDetails {
									owner,
									repo,
									number: *number,
								},
							)
						} else {
							None
						}
					})
					.or_else(|| {
						if let DetectUserCommentPullRequestPullRequest {
							html_url: Some(html_url),
						} = pr
						{
							parse_pull_request_details_from_url(html_url)
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

fn parse_pull_request_details_from_url(
	pr_html_url: &str,
) -> Option<PullRequestDetails> {
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
	Some(PullRequestDetails {
		owner,
		repo,
		number,
	})
}

/// full_name is org/repo
fn parse_repository_full_name(full_name: &str) -> Option<(String, String)> {
	let parts: Vec<&str> = full_name.split('/').collect();
	parts
		.first()
		.and_then(|owner| {
			parts.get(1).map(|repo_name| {
				Some((owner.to_string(), repo_name.to_string()))
			})
		})
		.flatten()
}

pub use client::GithubClient;
