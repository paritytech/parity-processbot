#![allow(non_upper_case_globals)]
use crate::{error, Result};
use parking_lot::RwLock;
use rocksdb::DB;
use serde::{de::DeserializeOwned, Serialize};
use snafu::ResultExt;
use std::sync::Arc;

pub enum CommitType {
	Update,
	Delete,
	None,
}

impl Default for CommitType {
	fn default() -> CommitType {
		CommitType::None
	}
}

pub trait DBEntry: Serialize + DeserializeOwned + Default {
	/// Add key to self.
	fn with_key(self, k: Vec<u8>) -> Self;

	/// Return an existing value if one exists, or else the default value.
	fn get_or_default(db: &Arc<RwLock<DB>>, k: Vec<u8>) -> Result<Self> {
		match db
			.read()
			.get_pinned(&k)
			.context(error::Db)?
			.map(|ref v| serde_json::from_slice::<Self>(v).context(error::Json))
		{
			Some(Ok(entry)) => Ok(entry),
			Some(e) => e,
			None => Ok(Self::default().with_key(k)),
		}
	}

	/// delete this entry from the db
	fn delete<K: AsRef<[u8]>>(
		&self,
		db: &Arc<RwLock<DB>>,
		key: K,
	) -> Result<()> {
		db.write().delete(&key).context(error::Db)
	}

	/// commit this entry to the db
	fn update<K: AsRef<[u8]>>(
		&self,
		db: &Arc<RwLock<DB>>,
		key: K,
	) -> Result<()> {
		{
			let db = db.write();
			db.delete(&key).context(error::Db)?;
			db.put(
				&key,
				serde_json::to_string(self).context(error::Json)?.as_bytes(),
			)
			.context(error::Db)
		}
	}
}
