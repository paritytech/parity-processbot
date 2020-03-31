FROM parity/rust-builder:latest as builder
WORKDIR /usr/src/parity-processbot
COPY . .
RUN cargo install --git https://github.com/paritytech/parity-processbot --branch master

FROM debian:buster-slim
RUN apt-get update && apt-get upgrade && apt-get install -y ca-certificates libssl-dev
COPY --from=builder /usr/local/cargo/bin/parity-processbot /usr/local/bin/parity-processbot
CMD ["parity-processbot"]
