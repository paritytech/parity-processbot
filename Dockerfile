FROM debian:buster-slim

COPY parity-processbot /usr/local/bin/parity-processbot

RUN set -ev; \
    apt-get update; \
    apt-get upgrade -y; \
    apt-get install -y --no-install-recommends \
        pkg-config curl ca-certificates libssl-dev clang git; \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
    git config --global user.name "parity-processbot"; \
    git config --global user.email "<>";

CMD ["parity-processbot"]
