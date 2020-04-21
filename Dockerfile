FROM parity/rust-builder:latest as builder
RUN cargo install --git https://github.com/paritytech/parity-processbot --branch master

FROM debian:buster-slim

RUN set -ev; \
    apt-get update; \
    apt-get upgrade -y; \
    apt-get install -y --no-install-recommends \
        pkg-config curl ca-certificates libssl-dev
COPY --from=builder /usr/local/cargo/bin/parity-processbot /usr/local/bin/parity-processbot
CMD ["parity-processbot"]
