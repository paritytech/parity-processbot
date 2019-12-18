use crate::developer::Developer;

pub trait Issue {
	fn assignee(&self) -> Option<Box<dyn Developer>>;
}
