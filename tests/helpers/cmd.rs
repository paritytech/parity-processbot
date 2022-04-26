use std::{
	ffi::OsStr,
	fmt::{Debug, Display},
	path::Path,
	process::{Command, Stdio},
};

pub enum CmdConfiguration<'a> {
	IgnoreStderrStartingWith(&'a [&'a str]),
}

fn build_cmd<Cmd, Dir>(cmd: Cmd, args: &[&str], dir: &Option<Dir>) -> Command
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	let mut cmd = Command::new(cmd);

	cmd.args(args);

	if let Some(dir) = dir {
		cmd.current_dir(dir);
		println!("Executing {:?} on {:?}", cmd, dir);
	} else {
		println!("Executing {:?}", cmd);
	}

	cmd
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
	let mut cmd = build_cmd(cmd, args, &dir);

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

pub fn get_cmd_success<Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
) -> bool
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	let mut cmd = build_cmd(cmd, args, &dir);
	cmd.spawn().unwrap().wait().unwrap().success()
}

pub fn get_cmd_output<Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
) -> String
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	let mut cmd = build_cmd(cmd, args, &dir);
	let output = cmd
		.stdout(Stdio::piped())
		.spawn()
		.unwrap()
		.wait_with_output()
		.unwrap();
	String::from_utf8_lossy(&output.stdout).trim().to_string()
}
