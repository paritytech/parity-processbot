use super::*;

use crate::{error::*, types::*};

impl Bot {
	pub async fn status<'a>(
		&self,
		args: StatusArgs<'a>,
	) -> Result<CombinedStatus> {
		let StatusArgs {
			owner,
			repo_name,
			sha,
		} = args;
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/status",
			base_url = self.base_url,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	pub async fn check_runs<'a>(
		&self,
		args: StatusArgs<'a>,
	) -> Result<CheckRuns> {
		let StatusArgs {
			owner,
			repo_name,
			sha,
		} = args;
		let url = format!(
			"{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs",
			base_url = self.base_url,
			owner = owner,
			repo = repo_name,
			sha = sha
		);
		self.client.get(url).await
	}

	pub async fn check_statuses(
		&self,
		db: &DB,
		commit_sha: &str,
	) -> Result<()> {
		if let Some(pr_bytes) = db.get(commit_sha.as_bytes()).context(Db)? {
			let m = bincode::deserialize(&pr_bytes).context(BincodeSnafu)?;
			log::info!("Deserialized merge request: {:?}", m);
			let MergeRequest {
				owner,
				repo_name,
				number,
				html_url,
				requested_by,
				head_sha,
			} = m;

			// Wait a bit for all the statuses to settle; some missing status might be
			// delivered with a small delay right after this is triggered, thus it's
			// worthwhile to wait for it instead of having to recover from a premature
			// merge attempt due to some slightly-delayed missing status.
			tokio::time::delay_for(std::time::Duration::from_millis(2048))
				.await;

			match self
				.pull_request(PullRequestArgs {
					owner,
					repo_name,
					number,
				})
				.await
			{
				Ok(pr) => {
					match pr.head_sha() {
						Ok(pr_head_sha) => {
							if head_sha != pr_head_sha {
								Err(Error::UnregisterPullRequest {
							  commit_sha: head_sha,
								msg: "HEAD commit changed before the merge could happen".to_string(),
							})
							} else {
								let statuses_state = self
									.get_latest_statuses_state(
										GetLatestStatusesStateArgs {
											owner,
											repo_name,
											commit_sha,
											html_url,
										},
									)
									.await;
								match statuses_state {
								Ok(outcome) => match outcome {
									Outcome::Success => match self
										.get_latest_checks(
												GetLatestChecksArgs {
														owner, repo_name, commit_sha, html_url
												}
										)
										.await
									{
										Ok(status) => match status {
											Outcome::Success => {
												merge(
													self,
													&owner,
													&repo_name,
													&pr,
													bot_config,
													&requested_by,
													None,
												)
												.await??;
												db.delete(&commit_sha)
													.context(Db)?;
												update_companion(
													self, &repo_name, &pr, db,
												)
												.await
											}
											Outcome::Failure => Err(
												Error::UnregisterPullRequest {
													commit_sha: commit_sha
														.to_string(),
													msg: "Statuses failed"
														.to_string(),
												},
											),
											_ => Ok(()),
										},
										Err(e) => Err(e),
									},
									Outcome::Failure => {
										Err(Error::UnregisterPullRequest {
											commit_sha: commit_sha.to_string(),
											msg: "Statuses failed".to_string(),
										})
									}
									_ => Ok(()),
								},
								Err(e) => Err(e),
							}
							}
						}
						Err(e) => Err(e),
					}
				}
				Err(e) => Err(e),
			}
			.map_err(|e| {
				e.map_issue(IssueDetails {
					owner,
					repo_name,
					number,
				})
			})?;
		}

		Ok(())
	}

	pub async fn get_latest_checks<'a>(
		&self,
		args: GetLatestChecksArgs<'a>,
	) -> Result<Outcome> {
		let GetLatestChecksArgs {
			owner,
			repo_name,
			commit_sha,
			html_url,
		} = args;
		let checks = self
			.check_runs(StatusArgs {
				owner,
				repo_name,
				commit_sha,
			})
			.await?;
		log::info!("{:?}", checks);

		// Since Github only considers the latest instance of each check, we should abide by the same
		// rule. Each instance is uniquely identified by "name".
		let mut latest_checks: HashMap<
			String,
			(usize, CheckRunStatus, Option<CheckRunConclusion>),
		> = HashMap::new();
		for c in checks.check_runs {
			if latest_checks
				.get(&c.name)
				.map(|(prev_id, _, _)| prev_id < &(&c).id)
				.unwrap_or(true)
			{
				latest_checks.insert(c.name, (c.id, c.status, c.conclusion));
			}
		}

		Ok(
			if latest_checks.values().all(|(_, _, conclusion)| {
				*conclusion == Some(CheckRunConclusion::Success)
			}) {
				log::info!("{} has successful checks", html_url);
				Outcome::Success
			} else if latest_checks
				.values()
				.all(|(_, status, _)| *status == CheckRunOutcome::Completed)
			{
				log::info!("{} has unsuccessful checks", html_url);
				Outcome::Failure
			} else {
				log::info!("{} has pending checks", html_url);
				Outcome::Pending
			},
		)
	}

	pub async fn get_latest_statuses_state<'a>(
		&self,
		args: GetLatestStatusesStateArgs<'a>,
	) -> Result<Outcome> {
		let GetLatestStatusesStateArgs {
			owner,
			owner_repo,
			commit_sha,
			html_url,
		} = args;
		let status = self
			.status(StatusArgs {
				owner,
				owner_repo,
				commit_sha,
			})
			.await?;
		log::info!("{:?}", status);

		// Since Github only considers the latest instance of each status, we should abide by the same
		// rule. Each instance is uniquely identified by "context".
		let mut latest_statuses: HashMap<String, (i64, StatusState)> =
			HashMap::new();
		for s in status.statuses {
			if s.description
				.as_ref()
				.map(|description| {
					match serde_json::from_str::<vanity_service::JobInformation>(
						description,
					) {
						Ok(info) => info.build_allow_failure.unwrap_or(false),
						_ => false,
					}
				})
				.unwrap_or(false)
			{
				continue;
			}
			if latest_statuses
				.get(&s.context)
				.map(|(prev_id, _)| prev_id < &(&s).id)
				.unwrap_or(true)
			{
				latest_statuses.insert(s.context, (s.id, s.state));
			}
		}
		log::info!("{:?}", latest_statuses);

		Ok(
			if latest_statuses
				.values()
				.all(|(_, state)| *state == StatusState::Success)
			{
				log::info!("{} has success status", html_url);
				Outcome::Success
			} else if latest_statuses
				.values()
				.any(|(_, state)| *state == StatusState::Pending)
			{
				log::info!("{} has pending status", html_url);
				Outcome::Pending
			} else {
				log::info!("{} has failed status", html_url);
				Outcome::Failure
			},
		)
	}
}
