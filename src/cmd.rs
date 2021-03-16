use crate::{error::*, Result};
use snafu::ResultExt;
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::process::{Output, Stdio};
use tokio::process::Command;

#[derive(PartialEq)]
pub struct CommandMessageConfiguration<'a> {
	pub secrets_to_hide: Option<&'a [&'a str]>,
	pub are_errors_silenced: bool,
}

#[derive(PartialEq)]
pub enum CommandMessage<'a> {
	Configured(CommandMessageConfiguration<'a>),
}

pub async fn run_cmd<'a, Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Dir,
	logging: CommandMessage<'a>,
) -> Result<Output>
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Display,
{
	before_cmd(&cmd, args, Some(&dir), &logging);

	#[allow(unused_mut)]
	let mut init_cmd = Command::new(cmd);
	let cmd = init_cmd.args(args).current_dir(dir).stderr(Stdio::piped());
	let result = cmd.output().await.context(Tokio)?;

	handle_cmd_result(cmd, result, &logging)
}

pub async fn run_cmd_in_cwd<'a, Cmd>(
	cmd: Cmd,
	args: &[&str],
	logging: CommandMessage<'a>,
) -> Result<Output>
where
	Cmd: AsRef<OsStr> + Display,
{
	before_cmd::<&Cmd, String>(&cmd, args, None, &logging);

	#[allow(unused_mut)]
	let mut init_cmd = Command::new(cmd);
	let cmd = init_cmd.args(args).stderr(Stdio::piped());
	let result = cmd.output().await.context(Tokio)?;

	handle_cmd_result(cmd, result, &logging)
}

pub async fn run_cmd_with_output<'a, Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Dir,
	logging: CommandMessage<'a>,
) -> Result<Output>
where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Display,
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

fn before_cmd<'a, Cmd, Dir>(
	cmd: Cmd,
	args: &[&str],
	dir: Option<Dir>,
	logging: &CommandMessage<'a>,
) where
	Cmd: AsRef<OsStr> + Display,
	Dir: AsRef<Path> + Display,
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
					cmd_display = cmd_display.replace(secret, "${SECRET}");
					args_display = args_display.replace(secret, "${SECRET}");
				}
			}

			if let Some(dir) = dir {
				log::info!("Run {} {} in {}", cmd_display, args_display, dir);
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

fn handle_cmd_result<'a>(
	cmd: &mut Command,
	result: Output,
	logging: &CommandMessage<'a>,
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
						cmd_display = cmd_display.replace(secret, "${SECRET}");
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
								err_output =
									err_output.replace(secret, "${SECRET}");
							}
						}
						log::error!("{}", err_output);
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
