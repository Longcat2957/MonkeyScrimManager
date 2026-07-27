# syntax=docker/dockerfile:1.7
FROM rust:1.96.1-bookworm AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY apps/api/Cargo.toml apps/api/Cargo.toml
COPY apps/riot-api/Cargo.toml apps/riot-api/Cargo.toml
RUN mkdir -p apps/api/src/bin \
    && printf 'pub fn placeholder() {}\n' >apps/api/src/lib.rs \
    && printf 'fn main() {}\n' >apps/api/src/main.rs \
    && printf 'fn main() {}\n' >apps/api/src/bin/migrate.rs
COPY apps/riot-api/src apps/riot-api/src
RUN --mount=type=cache,id=monkeybot-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=monkeybot-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=monkeybot-cargo-target,target=/workspace/target,sharing=locked \
    cargo build --locked --release --package monkeybot-riot-api --bin monkeybot-riot-api \
    && install -D target/release/monkeybot-riot-api /out/monkeybot-riot-api

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 monkeybot \
    && useradd --system --uid 10001 --gid monkeybot --home-dir /nonexistent --shell /usr/sbin/nologin monkeybot

COPY --from=builder /out/monkeybot-riot-api /usr/local/bin/monkeybot-riot-api

ENV MONKEYBOT_RIOT_BIND_ADDR=0.0.0.0:8081
EXPOSE 8081

USER 10001:10001
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8081/healthz >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/monkeybot-riot-api"]
