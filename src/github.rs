use crate::{constants::BOT_COMMANDS, error::*, Result, PR_HTML_URL_REGEX};
use regex::Regex;
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
	pub issues_url: Option<String>,
	pub pulls_url: Option<String>,
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
struct DetectUserCommentPullRequestRepository {
	pub name: Option<String>,
	pub full_name: Option<String>,
	pub owner: Option<User>,
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
							if let Some(
								DetectUserCommentPullRequestRepository {
									full_name: Some(full_name),
									..
								},
							) = repository
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
									.map(|(owner, name, _)| {
										(owner, name, *number)
									})
							} else {
								None
							}
						})
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

pub fn parse_issue_details_from_pr_html_url(
	pr_html_url: &str,
) -> Option<IssueDetails> {
	let re = Regex::new(PR_HTML_URL_REGEX!()).unwrap();
	let matches = re.captures(&pr_html_url)?;
	let owner = matches.name("owner")?.as_str().to_owned();
	let repo = matches.name("repo")?.as_str().to_owned();
	let number = matches
		.name("number")?
		.as_str()
		.to_owned()
		.parse::<usize>()
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

pub trait HasIssueDetails {
	fn get_issue_details(&self) -> Option<IssueDetails>;
}

impl HasIssueDetails for PullRequest {
	fn get_issue_details(&self) -> Option<IssueDetails> {
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

impl HasIssueDetails for Issue {
	fn get_issue_details(&self) -> Option<IssueDetails> {
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

pub fn owner_from_html_url(url: &str) -> Option<&str> {
	url.split("/").skip(3).next()
}

async fn merge_allowed(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
	requested_by: &str,
	min_approvals_required: Option<usize>,
) -> Result<Result<Option<String>>> {
	let is_mergeable = pr.mergeable.unwrap_or(false);

	if let Some(min_approvals_required) = &min_approvals_required {
		log::info!(
			"Attempting to reach minimum number of approvals {}",
			min_approvals_required
		);
	} else if is_mergeable {
		log::info!("{} is mergeable", pr.html_url);
	} else {
		log::info!("{} is not mergeable", pr.html_url);
	}

	if is_mergeable || min_approvals_required.is_some() {
		match github_bot.reviews(&pr.url).await {
			Ok(reviews) => {
				let mut errors: Vec<String> = Vec::new();

				// Consider only the latest relevant review submitted per user
				let mut latest_reviews: HashMap<usize, (&User, Review)> =
					HashMap::new();
				for review in reviews {
					// Do not consider states such as "Commented" as having invalidated a previous
					// approval. Note: this assumes approvals are not invalidated on comments or
					// pushes.
					if review
						.state
						.as_ref()
						.map(|state| {
							state != &ReviewState::Approved
								|| state != &ReviewState::ChangesRequested
						})
						.unwrap_or(true)
					{
						continue;
					}

					if let Some(user) = review.user.as_ref() {
						if latest_reviews
							.get(&user.id)
							.map(|(_, prev_review)| prev_review.id < review.id)
							.unwrap_or(true)
						{
							latest_reviews.insert(user.id, (user, review));
						}
					}
				}

				let team_leads = github_bot
					.substrate_team_leads(owner)
					.await
					.unwrap_or_else(|e| {
						let msg = format!(
							"Error getting {}: `{}`",
							SUBSTRATE_TEAM_LEADS_GROUP, e
						);
						log::error!("{}", msg);
						errors.push(msg);
						vec![]
					});

				let core_devs =
					github_bot.core_devs(owner).await.unwrap_or_else(|e| {
						let msg = format!(
							"Error getting {}: `{}`",
							CORE_DEVS_GROUP, e
						);
						log::error!("{}", msg);
						errors.push(msg);
						vec![]
					});

				let approvals = latest_reviews
					.iter()
					.filter(|(_, (user, review))| {
						review
							.state
							.as_ref()
							.map(|state| *state == ReviewState::Approved)
							.unwrap_or(false) && (team_leads
							.iter()
							.any(|team_lead| team_lead.login == user.login)
							|| core_devs
								.iter()
								.any(|core_dev| core_dev.login == user.login))
					})
					.count();

				let min_approvals_required = match repo_name {
					"substrate" => 2,
					_ => 1,
				};

				let has_bot_approved =
					latest_reviews.iter().any(|(_, (user, review))| {
						review
							.state
							.as_ref()
							.map(|state| {
								*state == ReviewState::Approved
									&& user
										.type_field
										.as_ref()
										.map(|type_field| {
											*type_field == UserType::Bot
										})
										.unwrap_or(false)
							})
							.unwrap_or(false)
					});

				let bot_approval = 1;
				// If the bot has already approved, then approving again will not make a difference.
				if !has_bot_approved
					&& approvals + bot_approval == min_approvals_required
				// Only attempt to pitch in the missing approval for team leads
					&& team_leads
						.iter()
						.any(|team_lead| team_lead.login == requested_by)
				{
					Ok(Some("a team lead".to_string()))
				} else {
					Ok(None)
				}
			}
			Err(e) => Err(e),
		}
	} else {
		Err(Error::Message {
			msg: format!("Github API says {} is not mergeable", pr.html_url),
		})
	}
	.map_err(|e| {
		e.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
	})
}

async fn ready_to_merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
) -> Result<bool> {
	match pr.head_sha() {
		Ok(pr_head_sha) => {
			match get_latest_statuses_state(
				github_bot,
				owner,
				repo_name,
				pr_head_sha,
				&pr.html_url,
			)
			.await
			{
				Ok(status) => match status {
					Status::Success => {
						match get_latest_checks_state(
							github_bot,
							owner,
							repo_name,
							pr_head_sha,
							&pr.html_url,
						)
						.await
						{
							Ok(status) => match status {
								Status::Success => Ok(true),
								Status::Failure => Err(Error::ChecksFailed {
									commit_sha: pr_head_sha.to_string(),
								}),
								_ => Ok(false),
							},
							Err(e) => Err(e),
						}
					}
					Status::Failure => Err(Error::ChecksFailed {
						commit_sha: pr_head_sha.to_string(),
					}),
					_ => Ok(false),
				},
				Err(e) => Err(e),
			}
		}
		Err(e) => Err(e),
	}
	.map_err(|e| {
		e.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
	})
}

async fn register_merge_request(
	owner: &str,
	repo_name: &str,
	number: usize,
	html_url: &str,
	requested_by: &str,
	commit_sha: &str,
	db: &DB,
) -> Result<()> {
	let m = MergeRequest {
		owner: owner.to_string(),
		repo_name: repo_name.to_string(),
		number: number,
		html_url: html_url.to_string(),
		requested_by: requested_by.to_string(),
	};
	log::info!("Serializing merge request: {:?}", m);
	let bytes = bincode::serialize(&m).context(Bincode).map_err(|e| {
		e.map_issue((owner.to_string(), repo_name.to_string(), number))
	})?;
	log::info!("Writing merge request to db (head sha: {})", commit_sha);
	db.put(commit_sha.trim().as_bytes(), bytes)
		.context(Db)
		.map_err(|e| {
			e.map_issue((owner.to_string(), repo_name.to_string(), number))
		})?;
	Ok(())
}
