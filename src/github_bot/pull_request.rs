use crate::{
	companion::CompanionReferenceTrailItem,
	error::Error,
	github::*,
	github_bot::MainConfig,
	webhook::{Dependency, MergeRequest},
	Result,
};

use super::GithubBot;

impl GithubBot {
	pub async fn pull_request(
		&self,
		owner: &str,
		repo: &str,
		number: i64,
	) -> Result<PullRequest> {
		self.client
			.get(format!(
				"{}/repos/{}/{}/pulls/{}",
				self.github_api_url, owner, repo, number
			))
			.await
	}

	pub async fn pull_request_with_head(
		&self,
		owner: &str,
		repo: &str,
		head: &str,
	) -> Result<Option<PullRequest>> {
		self.client
			.get_all(format!(
				"{}/repos/{}/{}/pulls?head={}",
				self.github_api_url, owner, repo, head
			))
			.await
			.map(|v| v.first().cloned())
	}

	pub async fn merge_pull_request(
		&self,
		owner: &str,
		repo: &str,
		number: i64,
		head_sha: &str,
	) -> Result<()> {
		let url = format!(
			"{}/repos/{}/{}/pulls/{}/merge",
			self.github_api_url, owner, repo, number
		);
		let params = serde_json::json!({
			"sha": head_sha,
			"merge_method": "squash"
		});
		self.client.put_response(&url, &params).await.map(|_| ())
	}

	pub async fn resolve_pr_dependents(
		&self,
		config: &MainConfig,
		pr: &PullRequest,
		requested_by: &str,
		companion_reference_trail: &[CompanionReferenceTrailItem],
	) -> Result<Option<Vec<MergeRequest>>, Error> {
		let companions =
			match pr.parse_all_companions(companion_reference_trail) {
				Some(companions) => companions,
				None => return Ok(None),
			};

		let parent_dependency = Dependency {
			sha: (&pr.head.sha).into(),
			owner: (&pr.base.repo.owner.login).into(),
			repo: (&pr.base.repo.name).into(),
			number: pr.number,
			html_url: (&pr.html_url).into(),
			is_directly_referenced: true,
		};
		let dependents =
			// If there's only one companion, then it can't possibly depend on another companion
			if let [(comp_html_url, comp_owner, comp_repo, comp_number)] =
				&*companions
			{
				let comp_pr = self
					.pull_request(comp_owner, comp_repo, *comp_number)
					.await?;
				vec![MergeRequest {
					was_updated: false,
					sha: comp_pr.head.sha,
					owner: comp_owner.into(),
					repo: comp_repo.into(),
					number: *comp_number,
					html_url: comp_html_url.into(),
					requested_by: requested_by.into(),
					dependencies: Some(vec![parent_dependency]),
				}]
			} else {
				let base_dependencies = vec![parent_dependency];

				let mut dependents = vec![];
				for (comp_html_url, comp_owner, comp_repo, comp_number) in &companions {
					// Prevent duplicate dependencies in case of error in user input
					if comp_repo == &pr.base.repo.owner.login {
						continue;
					}

					// Fetch the companion's lockfile in order to detect its dependencies
					let comp_pr = self
						.pull_request(comp_owner, comp_repo, *comp_number)
						.await?;
					let comp_lockfile = {
						let lockfile_content = self
							.contents(
								comp_owner,
								comp_repo,
								"Cargo.lock",
								&comp_pr.head.sha,
							)
							.await?;
						let txt_encoded = base64::decode(
							&lockfile_content.content.replace("\n", ""),
						)
						.map_err(|err| Error::Message {
							msg: format!(
								"Failed to decode the API content for the lockfile of {}/{}/pull/{}: {:?}",
								comp_owner, comp_repo, comp_number, err
							),
						})?;
						let txt = String::from_utf8_lossy(&txt_encoded);
						txt.parse::<cargo_lock::Lockfile>().map_err(|err| {
							Error::Message {
								msg: format!(
								"Failed to parse lockfile of {}/{}/pull/{}: {:?}",
								comp_owner, comp_repo, comp_number, err
							),
							}
						})?
					};

					let mut dependencies = base_dependencies.clone();

					// Go through all the other companions to check if any of them is a dependency
					// of this companion
					'to_next_other_companion: for (
						other_comp_html_url,
						other_comp_owner,
						other_comp_repo,
						other_comp_number,
					) in &companions
					{
						if other_comp_repo == comp_repo ||
							// Prevent duplicate dependencies in case of error in user input
							other_comp_repo == &pr.base.repo.owner.login {
							continue;
						}
						let other_comp_github_url = format!(
							"{}/{}/{}{}",
							config.github_source_prefix,
							other_comp_owner, other_comp_repo,
							config.github_source_suffix
						);
						for pkg in comp_lockfile.packages.iter() {
							if let Some(src) = pkg.source.as_ref() {
								if src.url().as_str() == other_comp_github_url {
									let other_comp_pr = self
										.pull_request(
											other_comp_owner,
											other_comp_repo,
											*other_comp_number,
										)
										.await?;
									dependencies.push(Dependency {
										owner: other_comp_owner.into(),
										repo: other_comp_repo.into(),
										sha: other_comp_pr.head.sha,
										number: *other_comp_number,
										html_url: other_comp_html_url.into(),
										is_directly_referenced: false
									});
									continue 'to_next_other_companion;
								}
							}
						}
					}

					dependents.push(MergeRequest {
						was_updated: false,
						sha: comp_pr.head.sha,
						owner: comp_owner.into(),
						repo: comp_repo.into(),
						number: *comp_number,
						html_url: comp_html_url.into(),
						requested_by: requested_by.into(),
						dependencies: Some(dependencies),
					})
				}

				dependents
			};

		log::info!("Dependents of {}: {:?}", pr.html_url, dependents);
		Ok(Some(dependents))
	}
}
