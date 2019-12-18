use crate::developer::Developer;

pub struct Project;

impl Project {
	pub fn owner(&self) -> Developer {}
	pub fn whitelist(&self) -> Vec<Developer> {}
        pub fn delegated_reviewer(&self) -> Developer {}
}
