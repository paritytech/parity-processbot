use crate::local_state::*;
use crate::{
	bots, constants::*, duration_ticks::DurationTicks, error, github, process,
	Result,
};
use itertools::Itertools;
use snafu::OptionExt;
use std::time::SystemTime;

impl bots::Bot {
	fn public_reviews_request(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		pull_request: &github::PullRequest,
	) -> Result<()> {
		let pr_html_url =
			pull_request.html_url.as_ref().context(error::MissingData)?;
		log::info!(
			"Requesting a review on {} from the project room.",
			pr_html_url
		);
		let ticks = local_state
			.reviews_requested_ping()
			.and_then(|ping| ping.elapsed().ok())
			.ticks(self.config.review_request_ping);
		match ticks {
			None => {
				local_state.update_reviews_requested_ping(
					Some(SystemTime::now()),
					&self.db,
				)?;
				local_state.update_reviews_requested_npings(1, &self.db)?;
				self.matrix_bot.send_to_room(
					&process_info.matrix_room_id,
					&REQUESTING_REVIEWS_MESSAGE
						.replace("{author}", &pull_request.user.login)
						.replace("{pr_url}", &pr_html_url),
				)?;
			}
			Some(0) => {}
			Some(i) => {
				if i > local_state.reviews_requested_npings() {
					local_state.update_reviews_requested_npings(i, &self.db)?;
					self.matrix_bot.send_to_room(
						&process_info.matrix_room_id,
						&REQUESTING_REVIEWS_MESSAGE
							.replace("{author}", &pull_request.user.login)
							.replace("{pr_url}", &pr_html_url),
					)?;
				}
			}
		}
		Ok(())
	}

	fn review_reminder(
		&self,
		local_state: &mut LocalState,
		process_info: &process::ProcessInfo,
		pull_request: &github::PullRequest,
		user: &github::User,
	) -> Result<()> {
		let pr_html_url =
			pull_request.html_url.as_ref().context(error::MissingData)?;

		// private message reminder
		{
			let elapsed = local_state
				.private_review_requested_from_user(&user.login)
				.and_then(|t| t.elapsed().ok());
			let private_ticks =
				elapsed.ticks(self.config.private_review_reminder_ping);
			match private_ticks {
                None => {
                    local_state.update_private_review_requested(user.login.clone(), SystemTime::now(), &self.db)?;
                    local_state.update_private_review_reminder_npings(user.login.clone(), 1, &self.db)?;
                    self.matrix_bot.message_mapped(
                        &self.db,
                        &self.github_to_matrix,
                        &user.login,
                        &PRIVATE_REVIEW_REMINDER_MESSAGE
                            .replace("{1}", &pr_html_url),
                    )?;
                }
                Some(0) => {},
                Some(i) => {
                    if &i > local_state.private_review_reminder_npings(&user.login).expect("should never set review_requested without also review_reminder_npings") {
                        local_state.update_private_review_reminder_npings(user.login.clone(), i, &self.db)?;
                        self.matrix_bot.message_mapped(
                            &self.db,
                            &self.github_to_matrix,
                            &user.login,
                            &PRIVATE_REVIEW_REMINDER_MESSAGE
                                .replace("{1}", &pr_html_url),
                        )?;
                    }
                }
            }
		}

		// public message reminder after some delay
		{
			let elapsed = local_state
				.public_review_requested_from_user(&user.login)
				.and_then(|t| t.elapsed().ok());
			let delay_ticks =
				elapsed.ticks(self.config.public_review_reminder_delay);
			match delay_ticks {
				Some(1..=std::u64::MAX) => {
					let public_ticks =
						elapsed.ticks(self.config.public_review_reminder_ping);
					match public_ticks {
                        None => {
                            local_state.update_public_review_requested(user.login.clone(), SystemTime::now(), &self.db)?;
                            local_state.update_public_review_reminder_npings(user.login.clone(), 1, &self.db)?;
                        }
                        Some(0) => {},
                        Some(i) => {
                            if &i > local_state.public_review_reminder_npings(&user.login).expect("should never set review_requested without also review_reminder_npings") {
                                local_state.update_public_review_reminder_npings(user.login.clone(), i, &self.db)?;
                                self.matrix_bot.send_to_room(
                                    &process_info.matrix_room_id,
                                    &PUBLIC_REVIEW_REMINDER_MESSAGE
                                        .replace("{1}", &pr_html_url)
                                        .replace("{2}", &user.login),
                                )?;
                            }
                        }
                    }
				}
				_ => {}
			}
		}

		Ok(())
	}

	pub async fn require_reviewers(
		&self,
		local_state: &mut LocalState,
		pull_request: &github::PullRequest,
		process_info: &process::ProcessInfo,
		reviews: &[github::Review],
		requested_reviewers: &github::RequestedReviewers,
	) -> Result<()> {
		let pr_html_url =
			pull_request.html_url.as_ref().context(error::MissingData)?;

		log::info!("Checking if reviews required on {}", pr_html_url);

		for review in reviews.iter().sorted_by_key(|r| r.submitted_at) {
			local_state.update_review(
				review.user.login.clone(),
				review.state.unwrap(),
				&self.db,
			)?;
		}
		// TODO check for code change and repeat

		for user in requested_reviewers.users.iter() {
			match local_state.review_from_user(&user.login) {
				None | Some(github::ReviewState::Pending) => {
					self.review_reminder(
						local_state,
						process_info,
						pull_request,
						user,
					)?;
				}
				_ => {}
			}
		}

		let reviewer_count = {
			let mut users = reviews
				.iter()
				.map(|r| &r.user)
				.chain(requested_reviewers.users.iter().by_ref())
				.collect::<Vec<&github::User>>();
			users.dedup_by_key(|u| &u.login);
			users.len()
		};

		let owner_or_delegate_requested = reviews
			.iter()
			.map(|r| &r.user)
			.chain(requested_reviewers.users.iter().by_ref())
			.any(|u| process_info.owner_or_delegate() == &u.login);

		let author_info = process_info.author_info(&pull_request.user.login);

		if !author_info.is_owner_or_delegate && !owner_or_delegate_requested {
			// author is not the owner/delegate and a review from the owner/delegate has not yet been
			// requested. request a review from the owner/delegate.
			log::info!(
				"Requesting a review on {} from the project owner.",
				pr_html_url
			);
			let github_login = process_info.owner_or_delegate();
			if let Some(matrix_id) = self.github_to_matrix.get(github_login) {
				self.matrix_bot.send_private_message(
					&self.db,
					&matrix_id,
					&REQUEST_DELEGATED_REVIEW_MESSAGE
						.replace("{1}", &pr_html_url),
				)?;
				self.github_bot
					.request_reviews(&pull_request, &[github_login.as_ref()])
					.await?;
			} else {
				log::error!(
                "Couldn't send a message to {}; either their Github or Matrix handle is not set in Bamboo",
                &github_login
            );
			}
		}

		if reviewer_count < self.config.min_reviewers {
			// post a message in the project's Riot channel, requesting a review;
			// repeat this message every 24 hours until a reviewer is assigned.
			self.public_reviews_request(
				local_state,
				process_info,
				pull_request,
			)?;
		}

		Ok(())
	}
}
