# 👾 Processbot

[![Processbot build status](https://circleci.com/gh/paritytech/parity-processbot.svg?style=svg)](https://app.circleci.com/github/paritytech/parity-processbot/pipelines)

A GitHub bot to automate common tasks and processes at parity.

## Development

### Dependencies

Processbot uses `rocksdb` to store state. `rocksdb` will try to build from
source by default. You can override this option by setting the `ROCKSDB_LIB_DIR`
environment variable to the directory containing the system rocksdb. This will
dynamically link to rocksdb. You can enable static linking with `ROCKSDB_STATIC=1`.

### Building

```
cargo build
```

### Configuration
Processbot looks for configuration variables in `.env` in the root directory.

`PRIVATE_KEY_PATH`: Path to the private key associated with the installed Processbot app. Eg. `PRIVATE_KEY_PATH=parity-processbot.2042-01-10.private-key.pem`.

`GITHUB_APP_ID`: App ID associated with the installed Processbot app. Eg. `GITHUB_APP_ID=12345`.

`DB_PATH`: Path to an existing `rocksdb` database or that path at which a database will be created. Eg. `DB_PATH=db`.

`MAIN_TICK_SECS`: Seconds between cycles of the main bot loop. Eg. `MAIN_TICK_SECS=900` (every 15 minutes).

`BAMBOO_TOKEN`: API Key used to access the BambooHR API. Eg. `BAMBOO_TOKEN=409f501eb797efdbb7ee8aff6adcb4654a98f8f3`.

`BAMBOO_TICK_SECS`: Seconds between updating data pulled from the BambooHR API. This can take some time and is likely to change only infrequently, so the value should be larger than `MAIN_TICK_SECS`. Eg. `BAMBOO_TICK_SECS=14400` (every 4 hours).

`MATRIX_HOMESERVER`: Matrix homeserver. Eg. `MATRIX_HOMESERVER=https://matrix.parity.io`.

`MATRIX_USER`: Email address associated with the bot's Matrix user. Eg. `MATRIX_USER=annoying_bot@parity.io`.

`MATRIX_PASSWORD`: Password associated with the bot's Matrix user. Eg. `MATRIX_PASSWORD=password123`.

`MATRIX_DEFAULT_CHANNEL_ID`: ID of a channel the bot should use when specific project details are unavailable. Eg.
`MATRIX_DEFAULT_CHANNEL_ID=!AcPNrbrUCYJqCNDPpU:matrix.parity.io`.

`STATUS_FAILURE_PING`: Seconds between notifications that a pull request has failed checks, sent privately to the pull request author, via Matrix.

`ISSUE_NOT_ASSIGNED_TO_PR_AUTHOR_PING`: Seconds between notifications that the issue relevant to a pull request has not been assigned to the author of the pull
request, sent privately to the issue assignee and project owner, then publicly to the project room, via Matrix.

`PROJECT_BACKLOG_COLUMN_NAME`: Name of the project column to which new issues should be attached.

`NO_PROJECT_AUTHOR_IS_CORE_PING`: Seconds between notifications that a pull request opened by a core developer has no project attached, sent privately to the
pull request author or publicly to the default channel if the author's Matrix handle cannot be found.

`NO_PROJECT_AUTHOR_IS_CORE_CLOSE_PR`: Seconds before closing a pull request opened by a core developer that has no project attached.

`NO_PROJECT_AUTHOR_NOT_CORE_CLOSE_PR`: Seconds before closing a pull request opened by an external developer that has no project attached.

`UNCONFIRMED_PROJECT_TIMEOUT`: Seconds before reverting an unconfirmed change of project by a non-whitelisted developer (currently unimplemented).

`MIN_REVIEWERS`: Minimum number of reviewers needed before a pull request can be accepted.

`REVIEW_REQUEST_PING`: Seconds between notifications requesting reviews on a pull request, sent publicly to the relevant project room, via Matrix.

`PRIVATE_REVIEW_REMINDER_PING`: Seconds between notifications reminding a reviewer to review a pull request, sent privately to the reviewer, via Matrix.

`PUBLIC_REVIEW_REMINDER_PING`: Seconds between notifications reminding a reviewer to review a pull request, sent publicly to the relevant project room, via Matrix.

`TEST_REPO_NAME`: Name of a Github repository to be used for testing.

### Testing

```
cargo test
```

To unit test the GithubBot: 
```
cargo test -- --ignored --test-threads=1
```
The `parity-processbot` app should be installed for the relevant organization and
`.env` should contain a valid `PRIVATE_KEY_PATH`, `GITHUB_APP_ID` and `TESTING_REPO_NAME`. Branch 
`testing_branch` should be ready to merge into `other_testing_branch`. 
