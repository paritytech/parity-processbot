use super::*;

use crate::{error::*, types::*};

impl Bot {
	pub async fn repository<'a>(
		&self,
		args: RepositoryArgs<'a>,
	) -> Result<Repository> {
		let RepositoryArgs { owner, repo_name } = args;
		let url = format!(
			"{base_url}/repos/{owner}/{repo_name}",
			base_url = self.base_url,
			owner = owner,
			repo_name = repo_name
		);
		self.client.get(url).await
	}
}
