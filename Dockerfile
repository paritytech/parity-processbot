FROM docker.io/paritytech/ci-linux:10a6c216-20200625

COPY parity-processbot /usr/local/bin/parity-processbot

RUN set -ev; \
    apt-get update; \
    apt-get upgrade -y; \
    apt-get install -y --no-install-recommends \
        pkg-config curl ca-certificates libssl-dev git; \
    git config --global user.name "parity-processbot"; \
    git config --global user.email "<>";

CMD ["parity-processbot"]
