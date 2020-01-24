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

### Testing

```
cargo test
```

To unit test the GithubBot: 
```
cargo test -- --ignored --test-threads=1
```
The `parity-processbot` app should be installed for the relevant organization and
`.env` should contain a valid `PRIVATE_KEY_PATH` and `TESTING_REPO_NAME`. Branch 
`testing_branch` should be ready to merge into `other_testing_branch`. 

