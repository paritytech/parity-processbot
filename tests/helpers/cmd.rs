use std::ffi::OsStr;
use std::fmt::Debug;
use std::fmt::Display;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

pub enum CmdConfiguration<'a> {
	IgnoreStderrStartingWith(&'a [&'a str]),
}

pub fn exec<Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
	conf: Option<CmdConfiguration<'_>>,
) where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	let mut cmd = Command::new(cmd);
	let cmd = {
		let cmd = cmd.args(args).stdout(Stdio::null());
		if let Some(dir) = dir {
			println!("Executing {:?} on {:?}", cmd, dir);
			cmd.current_dir(dir)
		} else {
			println!("Executing {:?}", cmd);
			cmd
		}
	};

	let was_success = match conf {
		Some(CmdConfiguration::IgnoreStderrStartingWith(
			prefixes_to_ignore,
		)) => {
			let out = cmd
				.stderr(Stdio::piped())
				.spawn()
				.unwrap()
				.wait_with_output()
				.unwrap();

			let err = String::from_utf8_lossy(&out.stderr);
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

			eprintln!("{}", err);

			out.status.success()
		}
		_ => cmd.spawn().unwrap().wait().unwrap().success(),
	};

	if !was_success {
		panic!("Command {:?} failed", cmd);
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
