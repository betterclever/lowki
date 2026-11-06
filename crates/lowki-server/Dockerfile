# syntax=docker/dockerfile:1
FROM rust:1.90-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin lowki-server && cp target/release/lowki-server /lowki-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /data lowki && mkdir -p /data && chown lowki /data
COPY --from=build /lowki-server /usr/local/bin/lowki-server
USER lowki
WORKDIR /data
ENV LOWKI_BIND=0.0.0.0:8080 \
    LOWKI_DATABASE_URL=sqlite:///data/lowki.db?mode=rwc \
    RUST_LOG=info
EXPOSE 8080
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=3s CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/lowki-server"]
