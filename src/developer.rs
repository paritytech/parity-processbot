pub trait Developer {
	fn user_id(&self) -> String;
	fn is_core_developer(&self) -> bool;
}
