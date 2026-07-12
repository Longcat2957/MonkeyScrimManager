FROM rust:1.96.1-bookworm AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY apps/api ./apps/api
RUN cargo build --locked --release --package monkeybot-api \
    --bin monkeybot-api \
    --bin monkeybot-migrate

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 monkeybot \
    && useradd --system --uid 10001 --gid monkeybot --home-dir /nonexistent --shell /usr/sbin/nologin monkeybot

COPY --from=builder /workspace/target/release/monkeybot-api /usr/local/bin/monkeybot-api
COPY --from=builder /workspace/target/release/monkeybot-migrate /usr/local/bin/monkeybot-migrate

ENV MONKEYBOT_BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

USER 10001:10001
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/healthz >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/monkeybot-api"]
