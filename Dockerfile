FROM rust
RUN apt-get --yes update && apt-get --yes install build-essential cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/mqtt-blink1
RUN apt update && apt install --yes gcc-arm-linux-gnueabihf
RUN rustup target add armv7-unknown-linux-gnueabihf
# https://stackoverflow.com/a/58474618
RUN echo "fn main() {}" > dummy.rs
COPY Cargo.toml .
RUN sed -i 's#src/main.rs#dummy.rs#' Cargo.toml
RUN cargo build --release --target armv7-unknown-linux-gnueabihf
RUN sed -i 's#dummy.rs#src/main.rs#' Cargo.toml
COPY . .
RUN cargo install --path .
# produces /usr/local/cargo/bin/mqtt-blink1
