use serde::{Deserialize, Serialize};

use crate::error::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderDeserializationItem {}
