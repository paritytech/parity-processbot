#!/bin/env sh

case "$1" in
	integration)
		git_daemon_pid_file="$(mktemp)"
		GIT_DAEMON_PID_FILE="$git_daemon_pid_file" cargo test --test '*'
		pid="$(cat "$git_daemon_pid_file")"
		while IFS= read -r line; do
			pids=()
			while [[ "$line" =~ [^\(]+\(([[:digit:]]+),[[:digit:]]+\) ]]; do
				kill "${BASH_REMATCH[1]}"
				line="${line:${#BASH_REMATCH[0]}}"
			done
			break
		done < <(pstree -p -g "$pid")
		rm "$git_daemon_pid_file"
		;;
	*)
		echo "Unknown parameter '$1'"
		exit 1
		;;
esac
