use std::{
	ffi::OsStr,
	fmt::{Debug, Display},
	path::Path,
	process::{Output, Stdio},
};

use snafu::ResultExt;
use tokio::process::Command;

use crate::{error::*, types::Result};

#[derive(PartialEq, Eq)]
pub struct CommandMessageConfiguration<'a, Secret: AsRef<str>> {
	pub secrets_to_hide: Option<&'a [Secret]>,
	pub are_errors_silenced: bool,
}

#[derive(PartialEq, Eq)]
pub enum CommandMessage<'a, Secret: AsRef<str>> {
	Configured(CommandMessageConfiguration<'a, Secret>),
}

pub async fn run_cmd<Cmd, Dir, Secret: AsRef<str>>(
	cmd: Cmd,
	args: &[&str],
	dir: Dir,
	logging: CommandMessage<'_, Secret>,
) -> Result<Output>
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	before_cmd(&cmd, args, Some(&dir), &logging);

	#[allow(unused_mut)]
	let mut init_cmd = Command::new(cmd);
	let cmd = init_cmd.args(args).current_dir(dir).stderr(Stdio::piped());
	let result = cmd.output().await.context(Tokio)?;

	handle_cmd_result(cmd, result, &logging)
}

pub async fn run_cmd_in_cwd<Cmd, Secret: AsRef<str>>(
	cmd: Cmd,
	args: &[&str],
	logging: CommandMessage<'_, Secret>,
) -> Result<Output>
where
	Cmd: AsRef<OsStr> + Display,
{
	before_cmd::<&Cmd, String, Secret>(&cmd, args, None, &logging);

	#[allow(unused_mut)]
	let mut init_cmd = Command::new(cmd);
	let cmd = init_cmd.args(args).stderr(Stdio::piped());
	let result = cmd.output().await.context(Tokio)?;

	handle_cmd_result(cmd, result, &logging)
}

pub async fn run_cmd_with_output<Cmd, Dir, Secret: AsRef<str>>(
	cmd: Cmd,
	args: &[&str],
	dir: Dir,
	logging: CommandMessage<'_, Secret>,
) -> Result<Output>
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	before_cmd(&cmd, args, Some(&dir), &logging);

	#[allow(unused_mut)]
	let mut init_cmd = Command::new(cmd);
	let cmd = init_cmd
		.args(args)
		.current_dir(dir)
		.stdin(Stdio::piped())
		.stderr(Stdio::piped());
	let result = cmd.output().await.context(Tokio)?;

	handle_cmd_result(cmd, result, &logging)
}

fn before_cmd<Cmd, Dir, Secret: AsRef<str>>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
	logging: &CommandMessage<Secret>,
) where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Debug,
{
	match logging {
		CommandMessage::Configured(CommandMessageConfiguration {
			secrets_to_hide,
			..
		}) => {
			let mut cmd_display = format!("{}", cmd);
			let mut args_display = format!("{:?}", args);
			if let Some(secrets) = secrets_to_hide.as_ref() {
				for secret in secrets.iter() {
					cmd_display =
						cmd_display.replace(secret.as_ref(), "${SECRET}");
					args_display =
						args_display.replace(secret.as_ref(), "${SECRET}");
				}
			}

			if let Some(dir) = dir {
				log::info!("Run {} {} in {:?}", cmd_display, args_display, dir);
			} else {
				log::info!(
					"Run {} {} in the current directory",
					cmd_display,
					args_display,
				);
			}
		}
	};
}

fn handle_cmd_result<Secret: AsRef<str>>(
	cmd: &mut Command,
	result: Output,
	logging: &CommandMessage<Secret>,
) -> Result<Output> {
	if result.status.success() {
		Ok(result)
	} else {
		let (cmd_display, err_msg) = match logging {
			CommandMessage::Configured(CommandMessageConfiguration {
				are_errors_silenced,
				secrets_to_hide,
			}) => {
				let mut cmd_display = format!("{:?}", cmd);
				if let Some(secrets) = secrets_to_hide.as_ref() {
					for secret in secrets.iter() {
						cmd_display =
							cmd_display.replace(secret.as_ref(), "${SECRET}");
					}
				}
				let err_msg = if *are_errors_silenced {
					None
				} else {
					let err_output = String::from_utf8_lossy(&result.stderr);
					if err_output.is_empty() {
						None
					} else {
						let mut err_output = err_output.to_string();
						if let Some(secrets) = secrets_to_hide.as_ref() {
							for secret in secrets.iter() {
								err_output = err_output
									.replace(secret.as_ref(), "${SECRET}");
							}
						}
						log::error!(
							"handle_cmd_result: {} failed with error: {}",
							cmd_display,
							err_output
						);
						Some(err_output)
					}
				};

				(cmd_display, err_msg)
			}
		};

		Err(Error::CommandFailed {
			cmd: cmd_display,
			status_code: result.status.code(),
			err: err_msg.unwrap_or_else(|| "no output".to_string()),
		})
	}
}
