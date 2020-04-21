FROM parity/rust-builder:latest as builder
RUN cargo install --git https://github.com/paritytech/parity-processbot --branch master

FROM debian:buster-slim

COPY --from=builder /usr/local/cargo/bin/parity-processbot /usr/local/bin/parity-processbot

RUN set -ev; \
    apt-get update; \
    apt-get upgrade -y; \
    apt-get install -y --no-install-recommends \
        pkg-config curl ca-certificates libssl-dev \
# apt clean up
	apt-get autoremove -y; \
	apt-get clean; \
	rm -rf /var/lib/apt/lists/*
CMD ["parity-processbot"]
