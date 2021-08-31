use crate::{error::*, types::*};
use rocksdb::DB;
use snafu::ResultExt;

pub async fn register_merge_request(db: &DB, mr: MergeRequest) -> Result<()> {
	log::info!("Serializing merge request: {:?}", &mr);
	let bytes = bincode::serialize(&mr).context(Bincode).map_err(|e| {
		e.map_issue(IssueDetails {
			owner,
			repo_name,
			number,
		})
	})?;
	log::info!("Writing merge request to db (head sha: {})", mr.head_sha);
	db.put(&mr.head_sha.trim().as_bytes(), bytes)
		.context(Db)
		.map_err(|e| {
			e.map_issue(IssueDetails {
				owner,
				repo_name,
				number,
			})
		})?;
	Ok(())
}
