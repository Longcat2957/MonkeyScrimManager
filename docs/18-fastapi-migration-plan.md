# Axum → FastAPI 완전 마이그레이션 계획

작성일: 2026-09-13

상태: 일반 내전 workflow 이관 진행 중. 현재 완료 범위와 검증 결과는 [실행 기록](migration/fastapi-progress.md)을 따른다.

현재 구현 단계의 검증 범위는 사용자 지시에 따라 백엔드로 한정한다. 격리 PostgreSQL 테스트, 계약/경쟁/오류 테스트, Docker 이미지 build·실행 검증으로 단계 완료를 판단하며, frontend 연결과 5173 proxy smoke 및 기존 로컬 Axum reload를 선행 조건으로 두지 않는다. 아래 Web/Bot 연결·운영 전환 절차는 이후 통합/배포 단계의 계획으로 남긴다.

현재 사용자 확정 기준: **동결한 현재 Axum/Rust 구현을 그대로 이관한다.** 문서와 구현이 충돌하면 현재 Rust handler/repository/test의 동작을 따른다. 이관 중 로직 개선·정책 변경을 섞지 않으며, 수정은 동등성 검증 후 별도 작업으로 수행한다.

## 추가 이관 예정 — 경매 기록 보존 마무리 (2026-09-26)

이번 사용자 지시는 경매 마무리 기능에 한해 Axum 백엔드와 Web을 먼저 수정하는 예외다.
FastAPI 구현 및 기존 동결 baseline 갱신은 이번 범위에 포함하지 않는다.

- [ ] Axum의 `GET /api/guilds/{guildId}/auction-tournaments/{id}/finish-preview`와
      `POST .../{id}/finish`를 이관한다. session 운영자·CSRF·revision·idempotency·감사/outbox 경계를 보존한다.
- [ ] SQLx 0026의 tournament/match `ended_early`와 terminal/lineup 제약을 인계한다.
      정상 완료의 승자 필수 조건은 유지하며 조기 완료만 무승자·미진행을 허용한다.
- [ ] 10인 Bo3/Bo5 중단, 동점, 경기 전 종료, 20인 준결승/결승 도중 종료를 지원한다.
      FINAL 스코어 선두만 우승팀으로 기록하고, 동점·결승 미진행은 우승팀 없이 완료한다.
- [ ] 확정 경기·MMR·라인업·BO 규칙 및 source 모집의 CONVERTED 상태를 보존한다.
      종료 후 모든 추가 진행/되돌리기를 거부하고 결과 조회·Discord 카드·전적의 무승부 표시를 맞춘다.
- [ ] 기존 FastAPI 결과 projection과 mutation에 남은 ‘COMPLETED는 반드시 승자가 있다’는 가정을
      제거한 뒤 신규 Axum 계약을 baseline/consumer matrix에 반영한다. 이 작업 전에는 신규 Web 마무리 흐름을
      FastAPI로 전환하지 않는다.

## 1. 목표와 완료 기준

현재 API의 동작과 PostgreSQL 데이터를 유지하면서 주 API, Riot 내부 API, migration 실행기를 Python으로 전환한다. Web과 Discord Bot의 기존 사용자 흐름을 유지하고, 운영·개발·테스트·배포 경로에서 Rust 실행 의존성을 제거한다.

다음 다섯 항목을 모두 만족해야 완료다.

1. 공개·내부 HTTP, OAuth, WebSocket, 조건부 개발 route, background 작업을 빠짐없이 이관한다.
2. Web/Bot이 사용하는 URL, 입력·출력, 인증, 권한, 오류, revision, idempotency 계약을 유지한다.
3. 기존 DB와 세션을 사용하며 경매 포인트·MMR·감사·outbox의 트랜잭션 정합성을 검증한다.
4. 로컬 reload, 배포, readiness, direct/proxy smoke, 장애 복구와 롤백을 실제 검증한다.
5. 안정화 이후 Axum/SQLx 실행기, Cargo 기반 빌드·테스트·배포 의존성을 제거한다. 과거 SQL과 결정 기록은 이력으로 보존한다.

이번 마이그레이션에 UI 재설계, 서비스 통합, DB 원장 재설계, 인증 방식 교체, 새 realtime 화면 확대를 섞지 않는다. 스키마 변경은 이관에 꼭 필요한 관리 메타데이터로 제한한다.

## 2. 현재 저장소에서 확인한 범위

아래 수치는 작성 시점 working tree의 정적 집계다. 배포된 바이너리의 기능 수나 최종 작업량을 뜻하지 않는다. 미커밋 API·Web·OpenAPI 변경이 있으므로 실행 착수 시 기준 commit과 추가 변경을 먼저 확정한다.

| 영역            | 확인한 상태                                           | 이관 범위                                                       |
| --------------- | ----------------------------------------------------- | --------------------------------------------------------------- |
| 주 API          | `apps/api/src` Rust 79개 파일, 67,543줄               | HTTP, 인증, repository, domain, PostgreSQL, realtime, bootstrap |
| 주 router       | `apps/api/src/http/mod.rs`의 `.route(` 96회           | method별 operation과 별도 router를 다시 집계해야 함             |
| API 통합 테스트 | `apps/api/tests` 26개 파일, 25,084줄                  | 시나리오를 언어 독립 계약 테스트와 Python 테스트로 이전         |
| Riot 서비스     | `apps/riot-api/src` 7개 파일, 2,794줄                 | health + 내부 API 4개, upstream client, cache, single-flight    |
| migration       | `apps/api/migrations`, `src/bin/migrate.rs`           | SQLx 이력 보존, Python migration 경로로 인계                    |
| 소비자          | `apps/web`, `apps/bot`                                | 호출 계약 유지, 실제 양쪽 소비자로 회귀 검증                    |
| 운영            | Compose dev/main, Dockerfile, systemd, nginx, scripts | 이미지·명령·probe·grant·backup 검증 변경                        |

소스 줄 수에는 inline unit test가 포함된다. 96이라는 수를 전체 endpoint 수로 사용하지 않는다. 복수 HTTP method, merge된 router, feature flag route, 내부 route, fallback을 포함한 실제 operation 목록은 P0 산출물이다.

주요 근거:

- 현재 기능: [운영 계약](12-current-operating-contract.md), [backend route](routing/backend.md), [실제 router](../apps/api/src/http/mod.rs).
- 업무 규칙: [예상 workflow](06-expected-workflows.md), [DB schema](05-db-schema.md), [realtime](07-realtime-collaboration.md).
- 기반 계약: [API 결정](decisions/0001-api-foundation.md), [PostgreSQL 결정](decisions/0002-postgresql-foundation.md).
- 전달 절차: [개발 속도](10-development-velocity.md), [배포](../deploy/README.md), [로컬 reload](../scripts/local-api-reload.sh).

문서와 구현의 차이는 계약 목록에 기록한다. 문서의 과거 migration 번호나 미완료 체크박스를 현재 구현 상태로 간주하지 않는다. 사용자 확정 지시에 따라 이관 중에는 현재 Rust 구현의 동작을 보존하고, 정책·정합성 개선은 이관 후 별도 변경으로 다룬다.

## 3. 권장 전환 방식

**별도 Python 구현 → 독립 DB에서 계약 비교 → staging 전체 전환 → 짧은 운영 점검 시간에 API 전체 교체 → 안정화 → Rust 제거** 순서를 권장한다.

단계별 개발은 업무 흐름 단위로 진행하되, 운영 write traffic을 미완성 Axum/FastAPI에 임의 분산하지 않는다. 일반 내전과 경매가 회원·시즌·MMR·idempotency·outbox를 공유하므로 URL별 분산은 이관 외에 새로운 운영 복잡성을 만든다.

- 이관 중 Axum이 운영 writer를 담당한다. Python은 독립 테스트 DB 또는 복원 DB를 사용한다.
- 동일 mutation을 두 구현에 비교 실행할 때는 동일 fixture에서 출발한 **서로 다른 DB**를 사용한다.
- 운영 요청을 shadow 실행하는 것은 필수가 아니다. 필요하면 부수효과 없는 GET만 명시적으로 허용한다. 세션 갱신·cache write·Riot 호출까지 확인하고, 실사용 토큰이나 개인정보를 fixture로 저장하지 않는다.
- 운영 전환 시 HTTP writer뿐 아니라 relay, import retention, Bot notification 소비 경로도 drain/중지 범위에 넣는다.
- API와 Riot 서비스의 네트워크 경계는 유지한다. 두 서비스를 각각 FastAPI로 바꾼다.

운영 중단 허용 시간과 목표 트래픽은 아직 확인되지 않았다. P0에서 실제 운영자와 확정한다. 무중단이 필수이면 세션·writer·worker 공존 검증과 트래픽 라우팅을 추가 설계하고 일정을 재산정한다.

## 4. 목표 기술과 코드 구조

아래는 채택 제안이다. 정확한 Python minor와 패키지 버전은 P1에서 호환성·이미지 빌드를 확인하고 lockfile에 고정한다.

| 용도        | 제안                                | 적용 원칙                                                     |
| ----------- | ----------------------------------- | ------------------------------------------------------------- |
| HTTP/ASGI   | FastAPI + Uvicorn                   | 기존 경로, middleware와 오류 계약 유지                        |
| DTO·설정    | Pydantic v2 + pydantic-settings     | camelCase alias, strict validation을 기존 입력 규칙에 맞춤    |
| DB          | SQLAlchemy 2.x async Core + asyncpg | 기존 SQL과 transaction 경계를 먼저 옮기고 ORM 재모델링은 보류 |
| Migration   | Alembic                             | SQLx baseline 검증 후 인계, runtime 자동 migration 금지       |
| 외부 HTTP   | HTTPX AsyncClient                   | lifespan에서 생성·종료, pool과 timeout 명시                   |
| Valkey      | redis-py asyncio client             | 기존 channel namespace와 payload 유지                         |
| 암호        | 검증된 cryptography 구현            | 기존 AES-GCM envelope와 hash 호환                             |
| 품질·패키징 | uv, pytest, Ruff, mypy              | 재현 가능한 install, 타입·계약·PostgreSQL 테스트              |

SQLAlchemy async는 task별 transaction/connection을 분리한다. ORM을 사용하는 부분이 생기더라도 `AsyncSession`을 concurrent task 사이에서 공유하지 않는다. [SQLAlchemy 공식 문서](https://docs.sqlalchemy.org/en/20/orm/extensions/asyncio.html)

전환 중 경로는 다음을 제안한다. 검증 종료 후 Python 앱을 기존 `apps/api`, `apps/riot-api` 경로로 이동한다.

```text
apps/api-python/
  pyproject.toml
  src/monkeybot_api/
    app.py                  # app factory, lifespan
    config.py
    http/                   # routers, DTO, dependencies, middleware, errors
    auth/                   # session, OAuth, CSRF, crypto
    domain/                 # ELO, enum, ID, 순수 규칙
    services/               # command orchestration, transaction 소유
    repositories/           # SQL 및 snapshot 조회
    infrastructure/         # DB, Valkey, HTTP client
    realtime/               # hub, subscription, relay
  migrations/               # Alembic baseline와 이후 revision
  tests/
apps/riot-api-python/
  pyproject.toml
  src/monkeybot_riot_api/
  tests/
tests/api-contract/         # base URL을 바꿔 두 구현에 적용
```

작업 중인 도메인에 필요한 파일만 만든다. 공통 generic repository나 포괄적인 내부 프레임워크를 먼저 만들지 않는다. 공개 응답 DTO와 DB row 모델을 분리하고, 한 command의 transaction은 service가 소유한다.

## 5. 반드시 유지할 계약

### 5.1 HTTP와 오류

- path/method, query 기본값, pagination·정렬, JSON field casing, enum, 누락과 `null`, status, header, redirect를 operation별로 비교한다.
- Discord snowflake는 canonical decimal string, domain ID는 기존 UUID 검증 및 UUIDv7 생성 계약을 유지한다.
- unknown field, 숫자 문자열·boolean 강제 변환, datetime 정밀도, Unicode 정규화·casefold, JSON 숫자 직렬화 차이를 golden fixture로 검사한다.
- Pydantic strict mode를 모든 입력에 일괄 적용하지 않는다. query는 문자열로 들어오므로 route별 기존 parser의 허용 범위에 맞춘다. [Pydantic 공식 문서](https://docs.pydantic.dev/latest/concepts/strict_mode/)
- FastAPI 기본 validation 응답을 그대로 노출하지 않는다. 기존 status와 `error.code`, 안전한 `error.message`, `details`, `requestId`로 변환한다. 404/405, 잘못된 JSON, content type, body limit, timeout, 미처리 예외도 포함한다. [FastAPI 오류 처리](https://fastapi.tiangolo.com/tutorial/handling-errors/)
- request마다 서버가 UUIDv7을 발급하고 `X-Request-ID`, 응답 envelope, log를 연결한다. 외부 request ID나 forwarded IP를 기존 정책보다 넓게 신뢰하지 않는다.
- 기존 보안·no-store header, 주 API의 기본 body 16 MiB, 요청 10초, drain 20초를 유지한다. Riot 서비스의 body limit은 현재 별도 16 KiB이므로 주 API 설정을 일괄 적용하지 않는다.
- HTTP timeout이 commit 취소를 보장한다고 가정하지 않는다. transaction 취소·connection 정리와 commit 후 응답 손실을 각각 테스트한다.
- canonical OpenAPI인 `openapi/monkeybot.yaml`을 기준으로 semantic diff한다. FastAPI 자동 생성 문서로 계약을 조용히 바꾸지 않는다.

### 5.2 인증·권한

- Discord OAuth, state/PKCE, callback redirect allowlist, 만료·재사용 방지, logout, session touch/revoke를 이전한다.
- 쿠키 `monkeybot_session`, `monkeybot_csrf`, `monkeybot_oauth_state`와 `x-csrf-token`, Path·TTL·SameSite·Secure·HttpOnly·host-only 속성을 보존한다. 중복 보안 쿠키 거부도 테스트한다.
- 세션 token/CSRF hash는 SHA-256 계약을 유지한다. JWT로 교체하지 않는다.
- 진행 중 OAuth도 이어갈 수 있도록 AES-256-GCM의 version + nonce + ciphertext/tag envelope, UUID bytes AAD, base64url 규칙을 양방향 fixture로 검증한다. Rust 발급→Python 검증과 Python 발급→Rust 검증을 모두 통과시킨다.
- user/Bot/service principal, active guild member/operator, 서비스 관리자와 환경 flag 경계를 유지한다. Web과 Bot이 같은 command를 호출해도 인증·CSRF 조건은 principal 계약대로 처리한다.
- 권한 검증과 입력 검증이 동시에 실패할 때 기존 오류 우선순위를 기록한다. FastAPI dependency 실행 순서에 따른 정보 노출 차이를 막는다.
- OAuth·Bot·Riot 외부 호출은 mock transport를 기본으로 테스트한다. 실제 Discord 메시지 발송은 별도 명시적으로 허용된 QA 범위에서만 수행한다.

### 5.3 DB·동시성·원장

- `guild_id` predicate와 composite FK 경계를 보존한다. 기존 설계에 없는 RLS 도입을 함께 진행하지 않는다.
- command별 lock 순서를 기록하고 `FOR UPDATE`, revision/state 조건, unique/check constraint를 그대로 보호한다.
- domain write + revision + idempotency + audit/checkpoint + outbox를 한 transaction으로 commit한다.
- 동일 revision 경쟁은 하나만 성공하고 실패 transaction은 원장과 revision을 바꾸지 않아야 한다.
- idempotency의 principal/method/route template/key scope, body hash 직렬화, advisory try-lock, in-progress·conflict·replay·expiry를 보존한다. 기존 Rust가 저장한 응답을 Python이 replay하고 그 반대도 검증한다.
- deadlock `40P01`만 기존 정책대로 전체 transaction을 최대 3회 재시도한다. commit 결과가 불확실한 연결 오류나 일반 timeout은 무조건 재시도하지 않는다.
- ELO의 기본 1500, K=32, lane matchup과 roster team/진영 구분, 기존 부동소수 계산·저장 방식을 유지한다. 반올림을 새로 추가하지 않는다. 고정 입력과 경계값으로 수치 오차를 측정하고 원장/API에서 허용 가능한 차이를 명시한다.
- pool의 UTC, search_path, application_name, statement/lock/idle-transaction timeout을 유지한다. worker 수 × pool 상한에 migration·운영 연결 여유를 더해 DB connection 예산을 정한다.
- availability, lease, expiry 판단에 사용하던 DB clock을 application clock으로 바꾸지 않는다.
- runtime/migration credential 분리, append-only ledger, 개발 reset 함수의 제한적 grant를 유지한다. readiness 성공만으로 권한 검증을 대체하지 않는다.

### 5.4 WebSocket·outbox·background 작업

- `/ws`는 invalidate만 전달하고 HTTP snapshot이 상태를 확정한다. 현재 허용된 topic과 active membership/operator/stage 조건을 유지한다.
- 정확히 하나의 일치하는 Origin, handshake 전 session 검증, 비인증 HTTP JSON 오류를 재현한다. ASGI 서버의 upgrade 전 응답 지원 여부를 실제 Uvicorn 조합으로 P1에서 검증한다.
- frame/message 8 KiB, socket당 topic 16개, 25초 ping·75초 idle, 느린 client와 권한 회수 처리, subscribe 전체 검증 및 ack 계약을 이관한다.
- bounded hub, `resync_required`, 중복·역순 event, reconnect/focus snapshot 복구를 보존한다.
- relay는 `SKIP LOCKED` claim → commit → publish → owner/token 일치 ack/nack 순서를 유지한다. 외부 I/O 중 row lock을 유지하지 않는다.
- Valkey 장애는 domain write를 rollback하지 않는다. backlog를 보존하고 readiness는 기존 degraded 정책을 유지한다.
- process별 hub/subscriber/relay와 import retention 작업을 lifespan으로 관리한다. API 시작 실패·정상 종료·worker cancellation 때 task와 connection이 남지 않아야 한다. [FastAPI lifespan](https://fastapi.tiangolo.com/advanced/events/)
- 최초 배포는 container당 Uvicorn worker 1개로 시작한다. 다중 instance 검증은 별도 수행하며, 여러 process 사용 시 공유 Valkey가 필요하다. local hub만으로 process 간 fan-out을 기대하지 않는다. [FastAPI 배포 개념](https://fastapi.tiangolo.com/deployment/concepts/)
- Bot notification outbox의 claim/ack/nack, lease, retry와 Bot heartbeat/status도 독립 체크리스트로 이관한다. Pub/Sub outbox와 혼동하지 않는다.

### 5.5 Riot 내부 API

- `/internal/riot/accounts/resolve`, `/records`, `/records/matches`, `/records/refresh`와 health를 이관한다.
- 내부 인증 header, 오류 mapping, API key의 서버 전용 보관, platform/routing, Riot ID/PUUID 검증을 유지한다.
- timeout, 429/Retry-After, upstream 장애, cache TTL, force refresh, 같은 key의 single-flight, 최근 경기 pagination과 시즌 조회 상한을 테스트한다.
- memory cache와 single-flight는 현재 process 범위임을 유지한다. 다중 worker가 upstream 호출을 배수로 늘릴 수 있으므로 초기 단일 worker에서 측정한다.
- 주 API의 Riot 계정 연결·해제·main 변경, 기록 privacy·refresh와 내부 Riot 서비스 변경을 함께 검증한다.

## 6. SQLx → Alembic 인계

애플리케이션 이관과 business schema 재설계를 분리한다. Alembic 도입 자체로 모든 기존 SQL을 다시 실행하지 않는다.

1. **Baseline 동결:** 전환 기준 SQL 파일 목록·checksum, `_sqlx_migrations`의 version/success/checksum, 실제 schema·index·constraint·function·grant를 manifest로 기록한다. 누락·변조·부분 적용은 인계 실패다.
2. **공존 기간:** SQLx one-shot migration을 유일한 DDL 소유자로 유지한다. 필요한 schema 변경은 기존 forward migration 한 경로로만 반영하고 두 구현을 갱신한다. Alembic과 SQLx를 동시에 DDL writer로 사용하지 않는다.
3. **기존 DB 인계:** P0에서 확정한 마지막 SQLx version까지 검증한 뒤 migration role로 Alembic baseline을 stamp한다. `stamp`는 migration을 실행하지 않으므로 schema 검사 없이 실행하지 않는다. 기존 `_sqlx_migrations`와 SQL checksum은 변경하거나 삭제하지 않는다. [Alembic stamp](https://alembic.sqlalchemy.org/en/latest/api/commands.html#alembic.command.stamp)
4. **빈 DB 구축:** 동일 baseline schema를 만드는 검토된 Alembic revision을 준비한다. table뿐 아니라 index, constraint, function, sequence/default, 필요한 extension을 포함한다. 별도 role provisioning/grant 실행까지 합쳐 기존 SQLx로 만든 빈 DB와 비교한다. `metadata.create_all()`만으로 대체하지 않는다.
5. **이력 구분:** 신규 Alembic DB에 과거 SQLx 실행 이력을 실행한 것처럼 꾸며 넣지 않는다. 기존 운영 DB는 SQLx 이력 + Alembic 인계, 신규 DB는 Alembic bootstrap provenance를 기록하고 readiness/backup 검증이 두 승인된 경로를 판별하게 한다.
6. **Readiness 변경:** 인계 전에는 exact SQLx manifest, 인계 후에는 baseline provenance와 예상 Alembic head 집합 및 보존 대상 legacy 이력을 검증한다. 알 수 없는 head·실패 이력은 503이다. 단순 DB ping이나 version 최댓값 확인으로 약화하지 않는다.
7. **권한 변경:** `runtime-grants.sql`의 전체 table grant 뒤 `alembic_version`과 인계 메타데이터도 runtime 쓰기를 revoke한다. SQLx 이력과 append-only 원장 보호를 유지한다.
8. **Rollback 기간:** business schema와 legacy SQLx ledger를 유지해 기존 운영 DB에서 Axum readiness와 실행이 가능해야 한다. Alembic 관리 테이블 추가를 포함한 양방향 기동을 staging에서 검증한다. 신규 Alembic-only DB에는 이 Axum rollback 보장을 적용하지 않는다.
9. **소유권 확정:** 안정화 후 SQLx 실행기를 제거하고 이후 DDL은 Alembic만 사용한다. 과거 SQL은 read-only 이력으로 보존하고 Python-only 이미지로 빈 DB 생성·기존 DB 인계·backup restore를 모두 검증한다.

rename/drop이 필요한 경우에는 별도 maintenance migration으로 분리한다. **모든 API instance 중지 → backup 확인 → migration → runtime grant → 해당 schema와 맞는 API 기동** 순서를 지킨다. 이번 전환은 그 변경이 필요 없도록 설계한다.

## 7. 단계별 작업과 통과 조건

각 행은 작업 묶음이며 하나의 대형 PR을 뜻하지 않는다. 실제 PR은 하나의 수직 흐름 또는 하나의 기반 변경으로 나눈다. 인원과 무관하게 파일별 담당자는 한 명으로 고정한다.

| 단계              | 작업                                                                                                               | 산출물·통과 조건                                                                 | 선행    |
| ----------------- | ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- | ------- |
| P0 계약 동결      | route/method/flag/소비자/권한/DTO/error/DB/worker 목록 작성, 기준 commit·schema 지정, 운영 기준 측정               | 누락 없는 operation matrix, 차이 목록, 성능 baseline, 전환·복구 목표             | 없음    |
| P1 기반           | Python 앱·패키지, 오류/middleware, DB pool, lifecycle, health/readiness, test harness                              | 실제 PostgreSQL로 runtime role 검사, 기존 envelope, lifespan·WS handshake 검증   | P0      |
| P2 인증·기본 조회 | OAuth/session/CSRF, user/guild context, membership/operator, profile/preferences/privacy                           | 기존 세션·OAuth 양방향 호환, tenant/privacy denial, Web SSR 로그인 흐름          | P1      |
| P3 일반 내전      | Web/Bot 모집 생성·참가·마감·재개·취소, entry, series, 픽밴, game/MMR, undo·종료                                    | 일반 내전 전체 흐름과 recovery, revision 경쟁, idempotency, outbox 정합성        | P2      |
| P4 경매           | 10/20인 모집·setup, bidding 7개 command, bracket, lineup, game/MMR, 취소·revert                                    | 각 format 흐름, point ledger·중복 낙찰·lineup barrier·취소/recovery 검증         | P3 기반 |
| P5 운영·데이터    | admin member/settings/MMR, season, audit, records/search/statistics/leaderboard, export/import, dummy/reset/status | privacy·filter·pagination, preview/apply atomicity, 환경 flag, archive 변환 호환 | P2~P4   |
| P6 Riot           | 별도 FastAPI Riot 앱, upstream/cache/single-flight, 주 API Riot integration                                        | 내부 4개 operation + 계정/기록 Web 흐름, upstream 실패·rate limit                | P1·P2   |
| P7 통합·운영 인계 | realtime 다중 instance/장애, notification 소비, Alembic 인계, 이미지·systemd·script 변경                           | Python 전체 계약, 빈 DB/복원 DB, 부하·drain·rollback rehearsal 통과              | P3~P6   |
| P8 운영 교체      | 점검 안내, writer/worker drain, 최종 검증·전환, smoke, 관측                                                        | FastAPI만 traffic 처리, 세션·Web/Bot·원장·outbox 정상                            | P7      |
| P9 안정화·삭제    | 관측 기간 통과, Python canonical 경로 확정, Cargo 제거, 문서 정리                                                  | Rust 없는 clean build/test/deploy/migrate/restore                                | P8      |

P6은 기반 완성 후 독립적으로 진행할 수 있으나, 일정에 병렬 인력을 가정하지 않는다. realtime·notification 코드는 P3/P4 command 이관 때 함께 작성하고 P7에서 처음 붙이지 않는다.

P3 필수 branch는 BO1/BO3/BO5, 하드 피어리스, 엔트리 reset, 마지막 game 되돌리기, 조기 종료다. P4는 10인과 20인, 준결승 전체 lineup lock 조건, `DRAW_NEXT`, `MARK_UNSOLD`, `AWARD_BID`, `ASSIGN_CURRENT`, `ASSIGN_UNSOLD`, `RESTORE_UNSOLD`, `REVERT_LAST`와 현재 router가 허용하는 recovery를 포함한다.

P5 import는 일반/전체 데이터 preview→apply, TTL·retention, 중복 적용, 다른 guild 데이터 차단, 실패 시 무변경을 검사한다. 고비용 parse·압축·직렬화는 event loop를 장시간 막는지 측정하고 필요한 부분만 bounded executor로 옮긴다.

## 8. 테스트 설계와 출시 gate

### 계약 테스트

`tests/api-contract`에서 base URL과 fixture DB를 바꿔 같은 시나리오를 양쪽에 적용한다. route registry의 모든 operation은 해당 suite 또는 domain별 테스트로 연결되어야 한다. 조건부 개발 route와 내부 route도 포함한다.

- 같은 초기 DB에서 요청 sequence를 실행하고 status/header/body와 DB 변화·원장·outbox를 비교한다.
- request ID·생성 시각·랜덤 ID는 위치가 지정된 비교 규칙으로만 정규화한다. ID 참조 관계, timestamp 유효성, revision, MMR·포인트는 임의로 제외하지 않는다.
- 권한·validation·conflict·dependency 오류는 machine code와 사용자 메시지를 모두 assertion한다.
- Rust의 기존 테스트는 Python 대체 시나리오가 연결된 뒤 제거한다. 대형 fixture를 줄이되 documented branch는 유지한다.
- 테스트가 mock HTTP에만 머무르지 않도록 Uvicorn 실제 process에서 cookie/redirect/body limit/WebSocket/proxy도 검증한다. frontend dev server를 새로 시작하지 않는다.

### 핵심 경쟁·장애 테스트

| 시나리오                           | 통과 조건                                                        |
| ---------------------------------- | ---------------------------------------------------------------- |
| 같은 revision으로 두 command       | 하나만 commit, 나머지는 기존 conflict 계약                       |
| 같은 idempotency key의 재시도      | 원장·알림 중복 없음, 저장 응답 replay                            |
| 같은 key에 다른 body               | 기존 conflict code/message, mutation 없음                        |
| 경매 낙찰 경쟁                     | 포인트·선수 배치·action/checkpoint가 일관됨                      |
| 결과 확정·되돌리기                 | game/score/MMR ledger atomicity, 기존 recovery 가능 조건 유지    |
| commit 후 response 손실            | snapshot/replay로 결과 확인, 이중 MMR 반영 없음                  |
| relay publish 전후 process 종료    | lease 만료 후 회복, stale token ack 거절, 허용된 중복 invalidate |
| Valkey 중단·복구                   | HTTP 원장 유지, backlog 회복·snapshot resync                     |
| WS 권한 회수·재연결·slow client    | 안전한 종료/재검증, 무권한 topic 유출 없음                       |
| Bot notification 재시도            | 기존 lease·ack 계약 유지, 발송 결과 불확실성 처리 유지           |
| DB lock timeout/deadlock/연결 단절 | 오류 안전화, 정의된 retry만 수행, connection 반환                |
| import 도중 실패                   | 부분 원장·부분 tenant 데이터가 남지 않음                         |

notification 전달을 exactly-once로 새로 약속하지 않는다. Discord 전송과 DB ack 사이 실패는 기존 복구·중복 방지 정책을 보존하고 재현한다.

### 성능·운영 gate

현재 Compose는 주 API 512 MiB, Riot 256 MiB 한도를 둔다. 같은 예산에서 session 조회, 목록/records, game submit, bid command, WS fan-out, 대용량 import를 측정한다. Python이 더 빠를 것이라고 가정하지 않는다.

P0에서 실제 지표를 수집하고 다음 제안값을 확정한다.

- 대표 동시 사용량과 그 2배 부하를 30분 유지한다. 실제 동시 사용량을 아직 모르므로 임의 RPS를 현황처럼 쓰지 않는다.
- 대표 route p95는 기존 대비 20% 초과 악화가 없고 기존 timeout 예산을 만족한다. p99도 기록해 tail latency 악화를 확인한다.
- 예상된 validation/conflict를 제외한 재현 가능한 5xx, 원장 불일치, tenant/privacy 실패는 0건이다.
- 메모리 한도 내 최소 20% 여유, pool 고갈 없음, outbox backlog가 장애 복구 후 사전에 정한 시간 안에 정상화된다.
- 오류율·latency·memory·DB pool·outbox oldest age·WS disconnect·Riot 429와 build revision을 관측한다. SLO와 rollback trigger 수치는 P0 측정으로 고정한다.

구현 중에는 변경 domain의 pytest/type/lint만 실행한다. 기반·migration·출시 gate에서 전체 Python suite와 필요한 Web/Bot check/test/build를 한 번 수행한다. Rust 최종 기준 suite도 기준 commit에서 한 번 기록하고 변경이 없으면 반복하지 않는다.

## 9. 개발·배포 파일 변경 계획

| 대상                          | 예정 변경                                                                                     |
| ----------------------------- | --------------------------------------------------------------------------------------------- |
| Python manifests/lock         | 재현 가능한 frozen install, Python runtime pin, runtime/test dependency 분리                  |
| API/Riot Dockerfile           | Python 이미지와 실행 명령, 비특권 runtime, signal 전달, health 도구                           |
| Compose dev/main              | 기존 service명·network·port·env 계약 유지, Python 이미지·migration entrypoint 반영            |
| local systemd                 | Uvicorn 실행과 working directory/env, 종료 제한, 로그 식별                                    |
| `scripts/local-api-reload.sh` | Rust build를 Python 검증/install/기동으로 교체하되 readiness + 30020 direct + 5173 proxy 유지 |
| build/rollout/preflight/smoke | Cargo·binary 경로 의존 제거, API/Riot/migration의 같은 release 추적                           |
| grant/backup/restore          | Alembic 및 baseline provenance 인식, 권한·원장 signature 검증 유지                            |
| Web/Bot config                | 내부 base URL, OAuth, cookie forwarding, error ID 전달, WS proxy를 회귀 확인                  |
| docs/AGENTS                   | 구현 계약 안정화 후 Axum/Cargo 전용 명령과 완료 조건을 FastAPI 기준으로 갱신                  |

nginx와 SvelteKit proxy가 어느 upstream을 보는지 전환 점검표에 각각 기록한다. 공개 HTTP만 전환되고 SSR 또는 Bot이 구 API를 계속 호출하는 상태를 허용하지 않는다.

실제 route 이관 후 로컬 완료 단계는 변경된 `scripts/local-api-reload.sh '/api/<changed-route>'`로 수행한다. 기존에 실행 중인 frontend만 사용한다. 401/403 등 route-level 응답은 smoke로 인정하지만 404·연결 실패·5xx는 실패다. 이 smoke는 인증된 workflow 테스트를 대신하지 않는다. Riot은 기존 `scripts/local-riot-smoke.sh`와 내부 호출 검증을 Python runtime에 맞춘다.

## 10. 운영 전환과 롤백 runbook

### 전환 전

1. 최종 기준 commit, 이미지 digest, schema manifest, env 설정 이름, 운영 담당자, 전환 창, 복구 목표를 기록한다. secret 값은 문서에 기록하지 않는다.
2. staging에서 운영과 같은 proxy/network/role 구성으로 P7을 통과한다.
3. 기존 backup 절차로 백업하고 격리 DB restore와 핵심 원장 signature를 확인한다.
4. FastAPI에서 write 후 Axum으로 복귀하는 rehearsal을 수행한다. 양방향 session·idempotency·checkpoint·outbox 호환을 확인한다.
5. 이전 API/Riot 이미지와 설정을 즉시 사용할 수 있게 보존한다. rollback 기간에는 business schema와 payload의 비호환 변경을 금지한다.

### 전환 실행

1. 새로운 Web mutation과 Bot workflow 진입을 점검 상태로 막는다. 사용자에게 점검 사유를 제공하고 client가 HTML/일반 오류로 오해하지 않도록 경계를 처리한다.
2. Bot notification consumer를 pause/drain하고, 기존 API의 in-flight HTTP·WS·relay·retention을 deadline 내 drain한 뒤 모든 구 API instance를 중지한다. 강제 종료된 lease와 결과 불명 요청 목록을 확인한다.
3. Riot 구 instance도 drain한다. DB backup/최종 signature와 기존 lease 상태를 확인한다.
4. 검증된 Alembic 인계만 실행하고 runtime grant를 적용한다. 실패하면 신규 API를 traffic에 넣지 않는다.
5. Python Riot와 API를 기동한다. liveness/readiness, 실제 실행 build revision, 내부 Riot 연결을 검증한다.
6. direct API와 기존 frontend proxy, OAuth redirect·기존 session, `/ws` handshake, Bot API 경로를 smoke한다. dev는 30020/5173, production은 해당 env의 실제 port/origin을 사용한다.
7. 승인된 QA 데이터로 일반 내전·경매·조회·notification 흐름을 확인하고 maintenance gate를 해제한다. consumer를 재개하고 pending lease/backlog 회복을 확인한다.
8. 첫 1시간 집중 관측 후 안정화 기간을 운영한다. 권장값은 최소 7일과 일반/경매 실제 운영 주기 각 1회 이상이며 P0에서 확정한다.

### 롤백

- tenant/privacy 위반, 원장 불일치, 반복 인증 실패, 핵심 command 불능은 즉시 중단 조건이다. latency/5xx/backlog는 P0에서 정한 임계값과 지속 시간으로 판단한다.
- 신규 mutation·consumer를 다시 막고 Python HTTP/worker를 drain·중지한 뒤 호환된 이전 API/Riot 이미지·설정을 복구한다. direct/proxy/auth/DB/outbox 검증 후 재개한다.
- 기본 롤백은 **현재 DB를 유지한 애플리케이션 복귀**다. FastAPI가 정상 처리한 운영 write를 보존한다. 이를 위해 양방향 저장 형식 검증이 P7 필수다.
- 불확실한 commit은 idempotency·snapshot·ledger로 확인한다. blind replay하지 않는다.
- DB restore는 애플리케이션 rollback과 구분한다. 실제 데이터 손상 시에만 정지·손상 범위 조사·RPO 및 이후 정상 write 보존/재적용 계획을 결정한 뒤 별도 복구 절차로 실행한다.
- 비호환 schema 변경을 넣으면 기존 이미지 복귀가 불가능할 수 있으므로 이 전환 창과 rollback 기간에는 배제한다.

## 11. 일정 추정과 실행 단위

다음은 저장소 규모에 기반한 **1인 전담 기준 초기 추정**이다. 실측 생산성, 기존 결함, 운영 QA 가용 시간은 반영되지 않았고 약속된 일정이 아니다. P0와 첫 수직 흐름 후 재산정한다.

| 작업                    | 예상 업무일               |
| ----------------------- | ------------------------- |
| P0 계약·baseline        | 3~5                       |
| P1 기반                 | 4~6                       |
| P2 인증·context         | 4~6                       |
| P3 일반 내전            | 7~10                      |
| P4 경매                 | 8~12                      |
| P5 운영·데이터          | 6~9                       |
| P6 Riot                 | 3~5                       |
| P7 통합·migration·운영  | 5~8                       |
| P8 전환 및 P9 삭제 작업 | 2~4                       |
| 합계                    | 42~~65 업무일, 약 9~~13주 |

안정화 관측 기간과 예외 대응 여유는 별도로 둔다. 일정 단축이 필요하면 독립된 Riot 또는 계약 fixture 작업에 담당자를 추가할 수 있지만 auth·transaction·같은 workflow 파일을 나눠 중복 소유하지 않는다.

착수 순서는 다음 세 작업으로 고정한다.

1. **작업 1:** 현재 working tree 변경을 포함할 기준을 결정하고 operation/worker/schema/consumer matrix와 계약 차이를 작성한다.
2. **작업 2:** Python 기반과 실제 PostgreSQL 테스트를 만들고 health/readiness, 공통 오류, transaction·role·WS handshake를 검증한다.
3. **작업 3:** 기존 session으로 `/api/me`와 guild context를 읽는 첫 흐름을 Web 소비자까지 연결한다. 이 결과로 나머지 일정·스택 선택을 재평가한다.

## 12. 최종 종료 체크리스트

- [ ] 모든 operation·flag route·worker가 Python 구현/테스트에 연결되어 있다.
- [ ] 기존 Web/Bot의 인증·오류·일반 내전·경매·관리·Riot 흐름이 통과한다.
- [ ] 기존 session, 진행 중 OAuth, idempotency replay, checkpoint와 원장이 양방향 호환된다.
- [ ] 빈 DB 구축, 기존 SQLx DB 인계, runtime grant, readiness, backup restore가 통과한다.
- [ ] 경쟁 mutation, timeout/commit 불확실성, 다중 instance relay/WS, Valkey 복구가 통과한다.
- [ ] 로컬 reload와 direct/proxy smoke, production proxy/SSR/Bot 경로가 같은 Python release를 확인한다.
- [ ] staging rollback rehearsal과 운영 안정화 기준을 통과했다.
- [ ] Python 앱을 canonical 경로로 이동하고 Docker/Compose/systemd/scripts/quality gate를 갱신했다.
- [ ] Rust 소스·Cargo manifests/lock·SQLx 실행기 의존을 제거해 Rust 없는 clean 환경에서 재현했다.
- [ ] 과거 migration SQL/ADR는 이력으로 보존하고 현재 운영 문서·AGENTS는 FastAPI 기준으로 갱신했다.

계획 작성 후 첫 구현 단위를 시작했다. 운영 중단 허용 시간, 트래픽/SLO, 최종 전환 기준 commit, 배포 DB의 실제 migration 상태는 P0에서 확인할 항목이다. Docker 후보 구현을 운영 전환 완료로 해석하지 않는다.
