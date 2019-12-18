use crate::developer::Developer;

pub trait Repository {
	fn project_owner(&self) -> Option<Box<dyn Developer>>;
	fn delegated_reviewer(&self) -> Option<Box<dyn Developer>>;
	fn whitelist(&self) -> Vec<Box<dyn Developer>>;
}
