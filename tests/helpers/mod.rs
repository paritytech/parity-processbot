use std::fs::{self, remove_dir_all, remove_file, File};
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

pub mod cmd;
pub mod constants;
pub mod setup;

use cmd::exec;

pub fn get_available_port() -> Option<u16> {
	for port in 1025..65535 {
		if TcpListener::bind(("127.0.0.1", port)).is_ok() {
			return Some(port);
		}
	}

	None
}

pub fn read_snapshot(log_dir: PathBuf, texts_to_hide: &[&str]) -> String {
	let entry = log_dir.read_dir().unwrap().next().unwrap().unwrap();
	let mut file = File::open(entry.path()).unwrap();
	let mut buf = String::new();
	file.read_to_string(&mut buf).unwrap();
	for text_to_hide in texts_to_hide.iter() {
		buf = buf.replace(text_to_hide, "{REDACTED}");
	}
	buf
}

pub fn clean_directory(dir: PathBuf) {
	for f in dir.read_dir().unwrap() {
		let f = f.unwrap();
		let _ = if f.metadata().unwrap().is_dir() {
			remove_dir_all(f.path())
		} else {
			remove_file(f.path())
		};
	}
}

pub fn initialize_repository(repo_dir: &Path, initial_branch: &str) {
	exec::<&str, PathBuf>(
		"git",
		&[
			"init",
			"--initial-branch",
			initial_branch,
			&repo_dir.display().to_string(),
		],
		None,
		None,
	);
	exec(
		"git",
		&["config", "--local", "user.name", "processbot"],
		Some(repo_dir),
		None,
	);
	exec(
		"git",
		&["config", "--local", "user.email", "foo@bar.com"],
		Some(repo_dir),
		None,
	);
	exec(
		"git",
		&["config", "--local", "advice.detachedHead", "false"],
		Some(repo_dir),
		None,
	);
	fs::write(&repo_dir.join("README"), "").unwrap();
	exec("git", &["add", "."], Some(&repo_dir), None);
	exec(
		"git",
		&["commit", "-m", "initial commit"],
		Some(&repo_dir),
		None,
	);
}
