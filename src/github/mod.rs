use crate::types::*;

mod bot;
mod commit;
mod companion;
mod http;
mod issue;
mod organization;
mod pull_request;
mod rebase;
mod repository;
mod review;
mod status;
mod team;
pub mod utils;

pub use bot::Bot;
pub use bot::Bot as GithubBot;

pub use utils::*;

pub struct WaitToMergeArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	number: &'a usize,
	html_url: &'a str,
	requested_by: &'a str,
	head_sha: &'a str,
}

pub struct PrepareToMergeArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	html_url: &'a str,
	number: &'a usize,
}

pub struct MergeArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	pr: &'a PullRequest,
	requested_by: &'a str,
	created_approval_id: Option<usize>,
}

pub struct MergeAllowedArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	pr: &'a PullRequest,
	requested_by: &'a str,
	min_approvals_required: Option<usize>,
}

pub struct CreateIssueCommentArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	body: &'a str,
	number: &'a usize,
}

pub struct StatusArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	sha: &'a str,
}

pub struct UpdateCompanionRepositoryArgs<'a> {
	owner: &'a str,
	owner_repo: &'a str,
	contributor: &'a str,
	contributor_repo: &'a str,
	contributor_branch: &'a str,
	merge_done_in: &'a str,
}

pub struct PerformCompanionUpdateArgs<'a> {
	html_url: &'a str,
	owner: &'a str,
	repo: &'a str,
	number: &'a usize,
	merge_done_in: &'a str,
}

pub struct PullRequestArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	number: &'a usize,
}

pub struct MergePullRequestArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	head_sha: &'a str,
	number: &'a usize,
}

pub struct ApproveMergeRequestArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	pr_number: &'a usize,
}

pub struct ClearBotApprovalArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	pr_number: &'a usize,
	review_id: &'a usize,
}

pub struct IsReadyToMergeArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	pr: &'a PullRequest,
}

pub struct OrgMembershipArgs<'a> {
	org: &'a str,
	username: &'a str,
}

pub struct RebaseArgs<'a> {
	base_owner: &'a str,
	base_repo: &'a str,
	head_owner: &'a str,
	head_repo: &'a str,
	branch: &'a str,
}

pub struct RepositoryArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
}

pub struct GetLatestChecksArgs<'a> {
	owner: &'a str,
	repo_name: &'a str,
	commit_sha: &'a str,
	html_url: &'a str,
}

pub struct TeamArgs<'a> {
	owner: &'a str,
	slug: &'a str,
}

pub struct GetLatestStatusesStateArgs<'a> {
	owner: &'a str,
	owner_repo: &'a str,
	commit_sha: &'a str,
	html_url: &'a str,
}
