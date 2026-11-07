FROM rust:1.91-bookworm AS build
WORKDIR /build
COPY central/Cargo.toml central/Cargo.lock ./
COPY central/src ./src
RUN cargo build --release --locked --bin cc-central
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 central
COPY --from=build /build/target/release/cc-central /usr/local/bin/cc-central
USER central
ENV BIND_ADDRESS=0.0.0.0:9001
EXPOSE 9001
ENTRYPOINT ["cc-central"]
