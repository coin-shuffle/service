FROM rust:1.67-buster as builder

WORKDIR /usr/src/service
COPY . .

RUN apt-get update \
    && apt-get install -y protobuf-compiler python3 python3-pip

RUN pip3 install solc-select \
    && solc-select install 0.8.19 \
    && solc-select use 0.8.19

# update cargo config to fetching with git cli
ENV CARGO_NET_GIT_FETCH_WITH_CLI true
RUN git config --global --add url."https://github.com/".insteadOf "git@github.com:"

Run cargo install --verbose --path .

FROM alpine:3.14
COPY --from=builder /usr/local/cargo/bin/service /usr/local/bin/service

ENV CONFIG /etc/config.toml

ENTRYPOINT ["service", "--config", "$CONFIG", "run"]
