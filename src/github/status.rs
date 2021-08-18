pub async fn check_statuses(
	github_bot: &GithubBot,
	db: &DB,
	commit_sha: &str,
) -> Result<()> {
	if let Some(pr_bytes) = db.get(commit_sha.as_bytes()).context(Db)? {
		let m = bincode::deserialize(&pr_bytes).context(Bincode)?;
		log::info!("Deserialized merge request: {:?}", m);
		let MergeRequest {
			owner,
			repo_name,
			number,
			html_url,
			requested_by,
		} = m;

		// Wait a bit for all the statuses to settle; some missing status might be
		// delivered with a small delay right after this is triggered, thus it's
		// worthwhile to wait for it instead of having to recover from a premature
		// merge attempt due to some slightly-delayed missing status.
		tokio::time::delay_for(std::time::Duration::from_millis(2048)).await;

		match github_bot.pull_request(&owner, &repo_name, number).await {
			Ok(pr) => match pr.head_sha() {
				Ok(pr_head_sha) => {
					if commit_sha != pr_head_sha {
						Err(Error::HeadChanged {
							expected: commit_sha.to_string(),
							actual: pr_head_sha.to_owned(),
						})
					} else {
						match get_latest_statuses_state(
							github_bot, &owner, &repo_name, commit_sha,
							&html_url,
						)
						.await
						{
							Ok(status) => match status {
								Status::Success => {
									match get_latest_checks(
										github_bot, &owner, &repo_name,
										commit_sha, &html_url,
									)
									.await
									{
										Ok(status) => match status {
											Status::Success => {
												merge(
													github_bot,
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
													github_bot, &repo_name,
													&pr, db,
												)
												.await
											}
											Status::Failure => {
												Err(Error::ChecksFailed {
													commit_sha: commit_sha
														.to_string(),
												})
											}
											_ => Ok(()),
										},
										Err(e) => Err(e),
									}
								}
								Status::Failure => Err(Error::ChecksFailed {
									commit_sha: commit_sha.to_string(),
								}),
								_ => Ok(()),
							},
							Err(e) => Err(e),
						}
					}
				}
				Err(e) => Err(e),
			},
			Err(e) => Err(e),
		}
		.map_err(|e| e.map_issue((owner, repo_name, number)))?;
	}

	Ok(())
}

pub async fn get_latest_checks(
	github_bot: &GithubBot,
	owner: &str,
	repo_name: &str,
	commit_sha: &str,
	html_url: &str,
) -> Result<Status> {
	let checks = github_bot
		.check_runs(&owner, &repo_name, commit_sha)
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
			Status::Success
		} else if latest_checks
			.values()
			.all(|(_, status, _)| *status == CheckRunStatus::Completed)
		{
			log::info!("{} has unsuccessful checks", html_url);
			Status::Failure
		} else {
			log::info!("{} has pending checks", html_url);
			Status::Pending
		},
	)
}
