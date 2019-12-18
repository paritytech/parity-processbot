use crate::developer::Developer;

pub trait Review {
	fn user(&self) -> Box<dyn Developer>;
	fn state(&self) -> String;
}
