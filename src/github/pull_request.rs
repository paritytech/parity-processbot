use super::*;
use crate::{error::*, types::*};

impl Bot {
	pub async fn pull_request<'a>(
		&self,
		args: PullRequestArgs<'a>,
	) -> Result<PullRequest> {
		let PullRequestArgs {
			owner,
			repo_name,
			pull_number,
		} = args;
		self.client
			.get(format!(
				"{base_url}/repos/{owner}/{repo}/pulls/{pull_number}",
				base_url = self.base_url,
				owner = owner,
				repo = repo_name,
				pull_number = pull_number
			))
			.await
	}

	pub async fn merge_pull_request<'a>(
		&self,
		args: MergePullRequestArgs<'a>,
	) -> Result<()> {
		let MergePullRequestArgs {
			owner,
			repo_name,
			number,
			head_sha,
		} = args;
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/pulls/{number}/merge",
			base_url = self.base_url,
			owner = owner,
			repo = repo_name,
			number = number,
		);
		let params = serde_json::json!({
			"sha": head_sha,
			"merge_method": "squash"
		});
		self.client.put_response(&url, &params).await.map(|_| ())
	}

	pub async fn approve_pull_request(
		&self,
		args: ApproveMergeRequestArgs<'a>,
	) -> Result<Review> {
		let ApproveMergeRequestArgs {
			owner,
			repo_name,
			pr_number,
		} = args;
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews",
			self.base_url, owner, repo_name, pr_number
		);
		let body = &serde_json::json!({ "event": "APPROVE" });
		self.client.post(url, body).await
	}

	pub async fn clear_bot_approval<'a>(
		&self,
		args: ClearBotApprovalArgs<'a>,
	) -> Result<Review> {
		let ClearBotApprovalArgs {
			owner,
			repo_name,
			pr_number,
			review_id,
		} = args;
		let url = &format!(
			"{}/repos/{}/{}/pulls/{}/reviews/{}/dismissals",
			self.base_url, owner, repo_name, pr_number, review_id
		);
		let body = &serde_json::json!({
			"message": "Merge failed despite bot approval, therefore the approval will be dismissed."
		});
		self.client.put(url, body).await
	}

	pub async fn merge_allowed<'a>(
		&self,
		args: MergeAllowedArgs<'a>,
	) -> Result<Result<Option<String>>> {
		let MergeAllowedArgs {
			owner,
			repo_name,
			pr,
			requested_by,
			min_approvals_required,
		} = args;

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
			match self.reviews(&pr.url).await {
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
								.map(|(_, prev_review)| {
									prev_review.id < review.id
								})
								.unwrap_or(true)
							{
								latest_reviews.insert(user.id, (user, review));
							}
						}
					}

					let team_leads = self
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
						self.core_devs(owner).await.unwrap_or_else(|e| {
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
								|| core_devs.iter().any(|core_dev| {
									core_dev.login == user.login
								}))
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
				msg: format!(
					"Github API says {} is not mergeable",
					pr.html_url
				),
			})
		}
		.map_err(|e| {
			e.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
		})
	}

	pub async fn is_ready_to_merge<'a>(
		&self,
		args: IsReadyToMergeArgs<'a>,
	) -> Result<bool> {
		let IsReadyToMergeArgs {
			owner,
			repo_name,
			pr,
		} = args;
		match pr.head_sha() {
			Ok(pr_head_sha) => {
				match get_latest_statuses_state(
					self,
					owner,
					repo_name,
					pr_head_sha,
					&pr.html_url,
				)
				.await
				{
					Ok(status) => match status {
						Status::Success => {
							match get_latest_checks(
								self,
								owner,
								repo_name,
								pr_head_sha,
								&pr.html_url,
							)
							.await
							{
								Ok(status) => match status {
									Status::Success => Ok(true),
									Status::Failure => {
										Err(Error::UnregisterPullRequest {
											commit_sha: pr_head_sha.to_string(),
											message: "Statuses failed",
										})
									}
									_ => Ok(false),
								},
								Err(e) => Err(e),
							}
						}
						Status::Failure => Err(Error::UnregisterPullRequest {
							commit_sha: pr_head_sha.to_string(),
							message: "Statuses failed",
						}),
						_ => Ok(false),
					},
					Err(e) => Err(e),
				}
			}
			Err(e) => Err(e),
		}
		.map_err(|e| {
			e.map_issue(IssueDetails {
				owner: owner.to_string(),
				repo: repo_name.to_string(),
				number: pr.number,
			})
		})
	}

	#[async_recursion]
	pub async fn merge<'a>(
		&self,
		args: MergeArgs<'a>,
	) -> Result<Result<(), MergeError>> {
		let MergeArgs {
			owner,
			repo_name,
			pr,
			requested_by,
			created_approval_id,
		} = args;
		match pr.head_sha() {
				Ok(pr_head_sha) => match self
					.merge_pull_request(owner, repo_name, pr.number, pr_head_sha)
					.await
				{
					Ok(_) => {
						log::info!("{} merged successfully.", pr.html_url);
						Ok(Ok(()))
					}
					Err(e) => match e {
						Error::Response {
							ref status,
							ref body,
						} => match *status {
							StatusCode::METHOD_NOT_ALLOWED => {
								match body.get("message") {
									Some(msg) => {
										// Matches the following
										// - "Required status check ... is {pending,expected}."
										// - "... required status checks have not succeeded: ... {pending,expected}."
										let missing_status_matcher = RegexBuilder::new(
											r"required\s+status\s+.*(pending|expected)",
										)
										.case_insensitive(true)
										.build()
										.unwrap();

										// Matches the following
										// - "At least N approving reviews are required by reviewers with write access."
										let insufficient_approval_quota_matcher =
											RegexBuilder::new(r"([[:digit:]]+).*approving\s+reviews?\s+(is|are)\s+required")
												.case_insensitive(true)
												.build()
												.unwrap();

										if missing_status_matcher
											.find(&msg.to_string())
											.is_some()
										{
											// This problem will be solved automatically when all the
											// required statuses are delivered, thus it can be ignored here
											log::info!(
												"Ignoring merge failure due to pending required status; message: {}",
												msg
											);
											Ok(Err(MergeError::MergeFailureWillBeSolvedLater { msg: msg.to_string() }))
										} else if let (
											true,
											Some(matches)
										) = (
											created_approval_id.is_none(),
											insufficient_approval_quota_matcher
												.captures(&msg.to_string())
										) {
											let min_approvals_required = matches
												.get(1)
												.unwrap()
												.as_str()
												.parse::<usize>()
												.unwrap();
											match merge_allowed(
												self,
												owner,
												repo_name,
												pr,
												requested_by,
												Some(min_approvals_required),
											)
											.await
											{
												Ok(result) => match result {
													Ok(requester_role) => match requester_role {
														Some(requester_role) => {
															let _ = self
																.create_issue_comment(
																	owner,
																	&repo_name,
																	pr.number,
																	&format!(
																		"Bot will approve on the behalf of @{}, since they are {}, in an attempt to reach the minimum approval count",
																		requested_by,
																		requester_role,
																	),
																)
																.await
																.map_err(|e| {
																	log::error!("Error posting comment: {}", e);
																});
															match self.approve_pull_request(
																owner,
																repo_name,
																pr.number
															).await {
																Ok(review) => merge(
																	self,
																	owner,
																	repo_name,
																	pr,
																	requested_by,
																	Some(review.id)
																).await,
																Err(e) => Err(e)
															}
														},
														None => Err(Error::Message {
															msg: "Requester's approval is not enough to make the PR mergeable".to_string()
														}),
													},
													Err(e) => Err(e)
												},
												Err(e) => Err(e),
											}.map_err(|e| Error::Message {
												msg: format!(
													"Could not recover from: `{}` due to: `{}`",
													msg,
													e
												)
											})
										} else {
											Err(Error::Message {
												msg: msg.to_string(),
											})
										}
									}
									_ => Err(Error::Message {
										msg: format!(
											"
		While trying to recover from failed HTTP request (status {}):

		Pull Request Merge Endpoint responded with unexpected body: `{}`",
											status, body
										),
									}),
								}
							}
							_ => Err(e),
						},
						_ => Err(e),
					}
					.map_err(|e| Error::MergeAttemptFailed {
						source: Box::new(e),
						commit_sha: pr_head_sha.to_string(),
						pr_url: pr.url.to_string(),
						owner: owner.to_string(),
						repo_name: repo_name.to_string(),
						pr_number: pr.number,
						created_approval_id
					}),
				},
				Err(e) => Err(e),
			}
			.map_err(|e| {
				e.map_issue((owner.to_string(), repo_name.to_string(), pr.number))
			})
	}

	pub async fn wait_to_merge<'a>(
		&self,
		args: WaitToMergeArgs<'a>,
		state: &AppState,
	) -> Result<()> {
		let WaitToMergeArgs {
			owner,
			repo_name,
			number,
			html_url,
			requested_by,
			head_sha,
		} = args;
		log::info!("{} checks incomplete.", html_url);
		register_merge_request(
			MergeRequest {
				owner,
				repo_name,
				number,
				html_url,
				requested_by,
				head_sha,
			},
			db,
		)
		.await?;
		log::info!("Waiting for commit status.");
		let _ = self
			.create_issue_comment(
				owner,
				&repo_name,
				number,
				"Waiting for commit status.",
			)
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
		Ok(())
	}

	pub async fn prepare_to_merge<'a>(
		&self,
		args: PrepareToMergeArgs<'a>,
	) -> Result<()> {
		let PrepareToMergeArgs {
			owner,
			repo_name,
			number,
			html_url,
		} = args;
		log::info!("{} checks successful; trying merge.", html_url);
		let _ = self
			.create_issue_comment(owner, &repo_name, number, "Trying merge.")
			.await
			.map_err(|e| {
				log::error!("Error posting comment: {}", e);
			});
		Ok(())
	}
}
