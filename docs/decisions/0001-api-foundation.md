# API Foundation Decisions

작성일: 2026-07-12

## Scope

Phase 1 Axum foundation에서 문서에 수치가 없던 설정을 다음처럼 고정한다.

## Runtime

- Rust toolchain: `1.96.1`
- API bind: `MONKEYBOT_BIND_ADDR`
- public origin: `MONKEYBOT_PUBLIC_ORIGIN`, production은 HTTPS만 허용
- request timeout: 기본 10초, `MONKEYBOT_REQUEST_TIMEOUT_MS`
- graceful drain: 기본 20초, `MONKEYBOT_SHUTDOWN_TIMEOUT_MS`
- request body: 기본 1 MiB, `MONKEYBOT_MAX_BODY_BYTES`

## Request ID

- Backend가 모든 요청마다 UUIDv7을 새로 발급한다.
- Header는 `X-Request-ID`다.
- 같은 값이 response header, success `meta.requestId`, error `error.requestId`, tracing span에 들어간다.
- Phase 1의 별도 분산 trace context는 아직 없으므로 structured log의 `trace_id`는 `request_id`와 같은
  값으로 기록한다. 후속 OpenTelemetry 도입 시 이 항목만 실제 trace ID로 교체한다.
- Phase 1에서는 외부 `X-Request-ID`와 forwarded client IP를 신뢰하지 않는다. Trusted proxy CIDR과
  forwarding 정책을 명시하기 전까지 client 입력을 그대로 사용하지 않는다.

## Health

- `/healthz`는 process liveness만 검사하며 dependency probe를 호출하지 않는다.
- `/readyz`의 core readiness는 PostgreSQL 연결과 migration version이다.
- Valkey 장애는 `degraded`지만 HTTP 원장이 동작하므로 200을 유지한다.
- SQLx 연결 전인 Phase 1 기본 probe는 PostgreSQL/migration `not_configured`와 503을 반환한다.
- Health 응답에는 DSN, raw dependency error, secret을 넣지 않는다.

## HTTP Boundary

- Axum extractor rejection을 포함한 모든 4xx/5xx, timeout, panic은 공통 JSON error envelope를
  사용한다. 원래 rejection 및 5xx 본문은 외부로 전달하지 않는다.
- error response는 response extension에 정규화된 `error_code`를 남기고 request completion log에도
  같은 값을 기록한다.
- API 응답은 `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`,
  `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`를 기본으로 둔다.
- Panic hook과 client response는 panic payload를 노출하지 않는다.

## Valkey And Locks

Phase 1에는 Valkey client나 분산 lock을 추가하지 않는다. 정합성은 PostgreSQL row lock, revision,
state condition, unique constraint로 보장하며 Valkey 연결은 Pub/Sub, presence, rate limit, cache가
필요해지는 후속 phase에서 추가한다.
