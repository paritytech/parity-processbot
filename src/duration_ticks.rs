pub trait DurationTicks {
	fn ticks(self, period: u64) -> Option<u64>;
}

impl DurationTicks for Option<std::time::Duration> {
	fn ticks(self, period: u64) -> Option<u64> {
		self.map(|s| s.as_secs() / period)
	}
}
