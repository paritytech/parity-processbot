use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

pub enum CmdConfiguration<'a> {
	SilentStderrStartingWith(&'a [&'a str]),
}

pub fn exec<Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
	conf: Option<CmdConfiguration<'_>>,
) where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path>,
{
	let mut cmd = Command::new(cmd);
	let cmd = {
		let cmd = cmd.args(args).stdout(Stdio::null());
		if let Some(dir) = dir {
			cmd.current_dir(dir)
		} else {
			cmd
		}
	};

	println!("Executing {:?}", cmd);

	match conf {
		Some(CmdConfiguration::SilentStderrStartingWith(
			prefixes_to_ignore,
		)) => {
			let out = cmd
				.stderr(Stdio::piped())
				.spawn()
				.unwrap()
				.wait_with_output()
				.unwrap();

			let err = String::from_utf8_lossy(&out.stdout);
			let err = err.trim();
			if err.is_empty() {
				return;
			} else {
				for prefix_to_ignore in prefixes_to_ignore {
					if err.starts_with(prefix_to_ignore) {
						return;
					}
				}
			};
			println!("STDERR: {}", err);
		}
		_ => {
			cmd.spawn().unwrap().wait().unwrap();
		}
	}
}

pub fn get_cmd_output<Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
) -> String
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path>,
{
	let mut cmd = Command::new(cmd);
	let cmd = {
		let cmd = cmd.args(args).stdout(Stdio::piped());
		if let Some(dir) = dir {
			cmd.current_dir(dir)
		} else {
			cmd
		}
	};

	println!("Getting output of {:?}", cmd);

	let output = cmd.spawn().unwrap().wait_with_output().unwrap();
	String::from_utf8_lossy(&output.stdout).trim().to_string()
}
