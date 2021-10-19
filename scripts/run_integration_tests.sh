#!/usr/bin/env sh

# GIT_DAEMON_BASE_PATH_TRACKER collects all the --base-path used for the Git
# daemon instances on tests and cleans them up when the tests end
git_daemon_base_path_tracker="$(mktemp)"

on_exit() {
	# kill lingering git daemon instances by their "base-path" argument because
	# the whole process tree is not finished when the main process exits;
	# targetting the process tree does not work either
	while IFS= read -r base_path; do
		>/dev/null pkill -f -- "--base-path=$base_path"
	done < "$git_daemon_base_path_tracker"

	rm "$git_daemon_base_path_tracker"
}
trap on_exit EXIT

# --test '*' means only run the integration tests
# https://github.com/rust-lang/cargo/issues/8396#issuecomment-713126649
# --nocapture is used so that we see the commands being executed interleaved within the logged info
GIT_DAEMON_BASE_PATH_TRACKER="$git_daemon_base_path_tracker" cargo test --test '*' -- --nocapture
