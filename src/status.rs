use crate::db::*;
use crate::local_state::*;
use crate::{
	bots, constants::*, error, github, matrix,
	process, Result,
};
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

		let owner_login = process_info.owner_or_delegate();
		let owner_or_delegate_approved = reviews
			.iter()
			.sorted_by_key(|r| r.submitted_at)
			.rev()
			.find(|r| &r.user.login == owner_login)
			.map_or(false, |r| r.state == Some(github::ReviewState::Approved));

		match status.state {
			github::StatusState::Failure => {
				// notify PR author by PM every 24 hours
				let should_ping = local_state.status_failure_ping().map_or(
					true,
					|ping_time| {
						ping_time.elapsed().ok().map_or(true, |elapsed| {
							elapsed.as_secs() > STATUS_FAILURE_PING_PERIOD
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
							&STATUS_FAILURE_NOTIFICATION
								.replace("{1}", &format!("{}", pr_html_url)),
						)?;
					} else {
						log::error!("Couldn't send a message to the project owner; either their Github or Matrix handle is not set in Bamboo");
					}
				}
			}
			github::StatusState::Success => {
				if owner_or_delegate_approved {
					// merge & delete branch
					self.github_bot
						.merge_pull_request(&repo.name, pr_number)
						.await?;
					local_state.delete(&self.db, &local_state.key)?;
				// TODO delete branch
				} else {
					local_state.update_status_failure_ping(None, &self.db)?;
				}
			}
			github::StatusState::Pending => {}
		}
		Ok(())
	}
}
