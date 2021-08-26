use crate::{github, Result};

use super::Bot;

impl Bot {
	pub async fn reviews(&self, pr_url: &str) -> Result<Vec<github::Review>> {
		let url = format!("{}/reviews", pr_url);
		self.client.get_all(url).await
	}
}
