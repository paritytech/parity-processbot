use crate::db::*;
use crate::local_state::*;
use crate::{bots, constants::*, error, github, matrix, process, Result};
use itertools::Itertools;
use snafu::OptionExt;
use std::time::SystemTime;

impl bots::Bot {
	pub async fn handle_status(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		repo: &github::Repository,
		pull_request: &github::PullRequest,
		status: &github::Status,
		reviews: &[github::Review],
	) -> Result<()> {
		let pr_number = pull_request.number.context(error::MissingData)?;
		let pr_html_url =
			pull_request.html_url.as_ref().context(error::MissingData)?;

		// the `mergeable` key is only returned with an individual GET request
		let pull_request = self.github_bot.pull_request(repo, pr_number).await?;

		if pull_request.mergeable.unwrap_or(false) {
			log::info!(
				"{} is mergeable; checking status.",
				pr_html_url
			);

			let owner_login = process_info.owner_or_delegate();
			let owner_or_delegate_approved = reviews
				.iter()
				.sorted_by_key(|r| r.submitted_at)
				.rev()
				.find(|r| &r.user.login == owner_login)
				.map_or(false, |r| {
					r.state == Some(github::ReviewState::Approved)
				});
			let core_dev_approvals = reviews
				.iter()
				.filter(|r| {
					self.core_devs.iter().any(|u| &u.login == &r.user.login)
						&& r.state == Some(github::ReviewState::Approved)
				})
				.count();
			let author_is_owner = pull_request
				.user
				.as_ref()
				.map(|u| &u.login == owner_login)
				.unwrap_or(false);

			match status.state {
				github::StatusState::Failure => {
					log::info!("{} failed checks.", pr_html_url);
					// notify PR author by PM every 24 hours
					let should_ping = local_state.status_failure_ping().map_or(
						true,
						|ping_time| {
							ping_time.elapsed().ok().map_or(true, |elapsed| {
								elapsed.as_secs()
									> self.config.status_failure_ping
							})
						},
					);

					if should_ping {
						local_state.update_status_failure_ping(
							Some(SystemTime::now()),
							&self.db,
						)?;
						if let Some(ref matrix_id) = self
							.github_to_matrix
							.get(owner_login)
							.and_then(|matrix_id| matrix::parse_id(matrix_id))
						{
							self.matrix_bot.send_private_message(
								&self.db,
								matrix_id,
								&STATUS_FAILURE_NOTIFICATION.replace(
									"{1}",
									&format!("{}", pr_html_url),
								),
							)?;
						} else {
							log::error!("Couldn't send a message to the project owner; either their Github or Matrix handle is not set in Bamboo");
						}
					}
				}
				github::StatusState::Success => {
					log::info!(
						"{} passed checks and can be merged.",
						pr_html_url
					);
					local_state.update_status_failure_ping(None, &self.db)?;
					if (author_is_owner
						&& core_dev_approvals >= self.config.min_reviewers)
						|| (!author_is_owner && owner_or_delegate_approved)
					{
						log::info!(
							"{} has necessary approvals; merging.",
							pr_html_url
						);
						// merge & delete branch
						self.github_bot
							.merge_pull_request(&repo.name, pr_number)
							.await?;
						local_state.delete(&self.db, &local_state.key)?;
					// TODO delete branch
					} else {
						log::info!(
							"{} does not have necessary approvals.",
							pr_html_url
						);
					}
				}
				github::StatusState::Pending => {
					log::info!("{} checks are pending.", pr_html_url);
					local_state.update_status_failure_ping(None, &self.db)?;
				}
			}
		} else {
			log::info!("{} is not mergeable.", pr_html_url);
		}
		Ok(())
	}
}
