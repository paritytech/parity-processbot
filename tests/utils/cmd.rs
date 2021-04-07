use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

pub enum CmdConfiguration<'a> {
	SilentStderrStartingWith(&'a [&'a str]),
}

pub fn run_cmd<'a, Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Dir,
	conf: Option<CmdConfiguration<'a>>,
) where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path>,
{
	let mut init_cmd = Command::new(cmd);
	let cmd = init_cmd.args(args).current_dir(dir).stdout(Stdio::null());

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
			if !err.is_empty() {
				for prefix_to_ignore in prefixes_to_ignore {
					if err.starts_with(prefix_to_ignore) {
						eprintln!("{}", err);
						break;
					}
				}
			};
		}
		_ => {
			cmd.spawn().unwrap().wait().unwrap();
		}
	}
}

pub fn get_cmd_output<Cmd, Dir>(cmd: Cmd, args: &[&str], dir: Dir) -> String
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path>,
{
	let out = Command::new(cmd)
		.args(args)
		.current_dir(dir)
		.stdout(Stdio::piped())
		.spawn()
		.unwrap()
		.wait_with_output()
		.unwrap();
	String::from_utf8_lossy(&out.stdout).trim().to_string()
}
