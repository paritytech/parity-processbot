#!/bin/env sh

git_daemon_base_path_file="$(mktemp)"

on_exit() {
	# kill lingering git daemon instances by their "base-path" argument because
	# the whole process tree is not finished when the main process exits;
	# targetting the process tree does not work either
	while IFS= read -r base_path; do
		>/dev/null pkill -f -- "--base-path=$base_path"
	done < "$git_daemon_base_path_file"

	rm "$git_daemon_base_path_file"
}
trap on_exit EXIT

# --test '*' means only run the integration tests
# https://github.com/rust-lang/cargo/issues/8396#issuecomment-713126649
GIT_DAEMON_BASE_PATH_FILE="$git_daemon_base_path_file" cargo test --test '*'
