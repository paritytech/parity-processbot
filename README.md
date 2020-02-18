# 👾 Processbot

[![Processbot build status](https://circleci.com/gh/paritytech/parity-processbot.svg?style=svg)](https://app.circleci.com/github/paritytech/parity-processbot/pipelines)

A GitHub bot to automate common tasks and processes at Parity.

## Repository Configuration 

### Project and Backlog 

- the repository must contain at least one project (matching a `[project-name]` key in `Process.toml`) 
- that project must contain a column named according to the `backlog` field of `Process.toml` below, if the field is included, otherwise named 'Backlog'.

### `Process.toml` file
Must be present in the repository's root directory. If it is absent Processbot will ignore the repository. 

##### `Process.toml` *must* contain:

- `[project-name]`:
  - must match a project in the repository
  - multiple projects can be listed, each with the fields below

- `owner = "github_user_login"`:
  - will be required to assign pull request authors to relevant issues
  - will be required to review new pull requests or assign reviewers
  - mergeable pull requests with the `owner`'s approval will be merged

- `whitelist = ["github_login0", "github_login1"]`:
  - may open pull requests without explicitly mentioning an issue 
  - will be warned before the pull request is closed for not having a project attached

- `matrix_room_id = "!SFhvpsdivdsds:matrix.example.io"`:
  - public notifications will be posted here

##### `Process.toml` *may* contain:

- `delegated_reviewer = "github_user_login"`
  - acts as the `owner`, intended as a stand-in when the `owner` will be unavailable for long periods

- `backlog = "column_name"`
  - project column to which new issues should be attached
  - will override the organization-wide value specified in `.env` (see below)

## Processbot Configuration

Processbot looks for configuration variables in `.env` in the root directory. Eg. `MATRIX_USER=annoying_bot@parity.io`.

`PRIVATE_KEY_PATH`: Path to the private key associated with the installed Processbot app.

`GITHUB_APP_ID`: App ID associated with the installed Processbot app.

`DB_PATH`: Path to an existing `rocksdb` database or that path at which a database will be created.

`MAIN_TICK_SECS`: Seconds between cycles of the main bot loop.

`BAMBOO_TOKEN`: API Key used to access the BambooHR API.

`BAMBOO_TICK_SECS`: Seconds between updating data pulled from the BambooHR API. This can take some time and is likely to change only infrequently, so the value should be larger than `MAIN_TICK_SECS`.

`MATRIX_SILENT`: If `true`, do not send Matrix notifications.

`MATRIX_HOMESERVER`: Matrix homeserver.

`MATRIX_USER`: Email address associated with the bot's Matrix user.

`MATRIX_PASSWORD`: Password associated with the bot's Matrix user.

`MATRIX_DEFAULT_CHANNEL_ID`: ID of a channel the bot should use when specific project details are unavailable.

`STATUS_FAILURE_PING`: Seconds between notifications that a pull request has failed checks, sent privately to the pull request author, via Matrix.

`ISSUE_NOT_ASSIGNED_TO_PR_AUTHOR_PING`: Seconds between notifications that the issue relevant to a pull request has not been assigned to the author of the pull
request, sent privately to the issue assignee and project owner, then publicly to the project room, via Matrix.

`NO_PROJECT_AUTHOR_IS_CORE_PING`: Seconds between notifications that a pull request opened by a core developer has no project attached, sent privately to the
pull request author or publicly to the default channel if the author's Matrix handle cannot be found.

`NO_PROJECT_AUTHOR_IS_CORE_CLOSE_PR`: Seconds before closing a pull request opened by a core developer that has no project attached.

`NO_PROJECT_AUTHOR_UNKNOWN_CLOSE_PR`: Seconds before closing a pull request opened by an external developer that has no project attached.

`PROJECT_CONFIRMATION_TIMEOUT`: Seconds before reverting an unconfirmed change of project by a non-whitelisted developer (currently unimplemented).

`MIN_REVIEWERS`: Minimum number of reviewers needed before a pull request can be accepted.

`REVIEW_REQUEST_PING`: Seconds between notifications requesting reviews on a pull request, sent publicly to the relevant project room, via Matrix.

`PRIVATE_REVIEW_REMINDER_PING`: Seconds between notifications reminding a reviewer to review a pull request, sent privately to the reviewer, via Matrix.

`PUBLIC_REVIEW_REMINDER_PING`: Seconds between notifications reminding a reviewer to review a pull request, sent publicly to the relevant project room, via Matrix.

`TEST_REPO_NAME`: Name of a Github repository to be used for testing.

## Dependencies

Processbot uses `rocksdb` to store state. `rocksdb` will try to build from
source by default. You can override this option by setting the `ROCKSDB_LIB_DIR`
environment variable to the directory containing the system rocksdb. This will
dynamically link to rocksdb. You can enable static linking with `ROCKSDB_STATIC=1`.

## Building

```
cargo build
```

## Testing

```
cargo test
```

#### To test Github queries: 
```
cargo test -- --ignored --test-threads=1
```

This requires:
- Processbot installed for the organization owning the testing repo
- `.env` contain a valid `PRIVATE_KEY_PATH`, `GITHUB_APP_ID` and `TESTING_REPO_NAME`
- branch `testing_branch` ready to merge into `other_testing_branch`
