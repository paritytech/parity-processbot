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

To test the GithubBot: 
```
cargo test -- --ignored --test-threads=1
```
make sure `GITHUB_ORGANIZATION` and `GITHUB_TOKEN` are set correctly in `.env`.

