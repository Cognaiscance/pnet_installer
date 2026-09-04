FROM rust:1-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /build/target/release/pnet_installer /usr/local/bin/pnet_installer
CMD ["pnet_installer"]
