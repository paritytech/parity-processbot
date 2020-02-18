use crate::{
	bots, constants::*, error, github, local_state::*, matrix, Result,
};
use snafu::OptionExt;
use std::time::SystemTime;

impl bots::Bot {
	async fn status_ping(
		&self,
		local_state: &mut LocalState,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		// notify PR author by PM at regular intervals
		let should_ping =
			local_state.status_failure_ping().map_or(true, |ping_time| {
				ping_time.elapsed().ok().map_or(true, |elapsed| {
					elapsed.as_secs() > self.config.status_failure_ping
				})
			});

		if should_ping {
			local_state.update_status_failure_ping(
				Some(SystemTime::now()),
				&self.db,
			)?;
			if let Some(ref matrix_id) = self
				.github_to_matrix
				.get(&pull_request.user.login)
				.and_then(|matrix_id| matrix::parse_id(matrix_id))
			{
				self.matrix_bot.send_private_message(
					&self.db,
					matrix_id,
					&STATUS_FAILURE_NOTIFICATION
						.replace("{1}", &pull_request.html_url),
				)?;
			} else {
				log::error!(
                    "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
                    &pull_request.user.login
                );
			}
		}

		Ok(())
	}

	pub async fn handle_status(
		&self,
		local_state: &mut LocalState,
		pull_request: &github::PullRequest,
		status: &github::CombinedStatus,
	) -> Result<()> {
		if pull_request.mergeable.unwrap_or(false) {
			log::debug!(
				"{} is mergeable; checking status.",
				pull_request.html_url
			);

			if status.total_count > 0 {
				match status.state {
					github::StatusState::Failure => {
						log::debug!("{} failed checks.", pull_request.html_url);
						self.status_ping(local_state, pull_request).await?;
					}
					github::StatusState::Success => {
						log::debug!(
							"{} passed checks and can be merged.",
							pull_request.html_url
						);
						local_state
							.update_status_failure_ping(None, &self.db)?;
					}
					github::StatusState::Pending => {
						log::debug!(
							"{} checks are pending.",
							pull_request.html_url
						);
						local_state
							.update_status_failure_ping(None, &self.db)?;
					}
				}
			} else {
				log::debug!(
					"{} has no checks and can be merged.",
					pull_request.html_url
				);
			}
		} else {
			log::debug!("{} is not mergeable.", pull_request.html_url);
			self.status_ping(local_state, pull_request).await?;
		}

		Ok(())
	}
}
