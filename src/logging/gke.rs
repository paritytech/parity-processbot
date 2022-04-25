// GKE stands for Google Kubernetes Engine

use std::io::{self, Write};

use env_logger::fmt::Formatter;
use log::Record;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "UPPERCASE")]
enum Severity {
	Error,
	Info,
}

#[derive(Serialize)]
struct Log {
	pub severity: Severity,
	pub message: String,
	pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub fn format(fmt: &mut Formatter, record: &Record) -> io::Result<()> {
	writeln!(
		fmt,
		"{}",
		serde_json::to_string(&Log {
			severity: match record.level() {
				log::Level::Error => Severity::Error,
				_ => Severity::Info,
			},
			message: format!("{}", record.args()),
			timestamp: chrono::Utc::now(),
		})
		.unwrap_or_else(|_| format!(
			"ERROR: Unable to serialize {}",
			record.args()
		))
	)
}
