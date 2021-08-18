#[derive(Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct MergeRequest {
	owner: String,
	repo_name: String,
	number: usize,
	html_url: String,
	requested_by: String,
}

pub async fn merge_allowed(
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

pub async fn is_ready_to_merge(
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
						match get_latest_checks(
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

pub async fn register_merge_request(
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

#[async_recursion]
pub async fn merge(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	pr: &PullRequest,
	requested_by: &str,
	created_approval_id: Option<usize>,
) -> Result<Result<(), MergeError>> {
	match pr.head_sha() {
		Ok(pr_head_sha) => match github_bot
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
									Ok(Err(Error::MergeFailureWillBeSolvedLater { msg: msg.to_string() }))
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
										github_bot,
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
													let _ = github_bot
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
													match github_bot.approve_merge_request(
														owner,
														repo_name,
														pr.number
													).await {
														Ok(review) => merge(
															github_bot,
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
			.map_err(|e| Error::Merge {
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
