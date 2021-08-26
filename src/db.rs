use crate::types::*;
use rocksdb::DB;

pub async fn register_merge_request(mr: MergeRequest, db: &DB) -> Result<()> {
	log::info!("Serializing merge request: {:?}", &mr);
	let bytes = bincode::serialize(mr).context(Bincode).map_err(|e| {
		e.map_issue((owner.to_string(), repo_name.to_string(), number))
	})?;
	log::info!("Writing merge request to db (head sha: {})", &mr.head_sha);
	db.put(&mr.head_sha.trim().as_bytes(), bytes)
		.context(Db)
		.map_err(|e| {
			e.map_issue((owner.to_string(), repo_name.to_string(), number))
		})?;
	Ok(())
}
