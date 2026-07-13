# PostgreSQL Foundation Decisions

작성일: 2026-07-12

## Scope

Axum API의 PostgreSQL 기반과 Phase 2 이후 repository가 따라야 할 저장소 계약을 고정한다.
상세 table과 transaction 규칙은 `docs/05-db-schema.md`, 실행 순서는
`docs/09-backend-development-plan.md`를 따른다.

## Version And Ownership

- PostgreSQL은 Docker Official Image의 exact tag `18.4-alpine3.23`으로 고정한다.
- PostgreSQL만 영속 상태와 revision의 source of truth다.
- Valkey는 cache, presence, rate limit, Pub/Sub 용도이며 원장으로 사용하지 않는다.
- UUID domain ID는 application에서 UUIDv7으로 생성한다. Discord snowflake는 canonical unsigned
  decimal string으로 검증해 저장한다.
- status와 lane은 PostgreSQL enum 대신 `text + CHECK`를 사용하고 application boundary에서는 typed
  value로 다룬다.

## Migration

- `0001`부터 `0009`까지 SQLx forward migration을 순서대로 적용한다.
- API process는 startup migration을 실행하지 않는다.
- 배포 전 `monkeybot-migrate` one-shot process가 별도 migration credential로 migration을 적용한다.
- 기본 배포는 expand-contract migration만 허용한다. 실행 중인 binary가 참조하는 column을 rename/drop하는
  migration은 rolling 배포와 섞지 않고 모든 API instance를 내린 maintenance window에서만 적용한다.
- `0012_user_identity_kinds.sql`은 `users.discord_id`를 `users.user_id`로 바꾸는 명시적 maintenance
  migration이다. 이 migration을 포함한 최초 배포는 새 image를 먼저 준비한 뒤 API 전체 중지, backup
  확인, migration, runtime grant, 새 API 기동 순서를 지켜야 한다.
- `/readyz`는 연결 성공만으로 준비 완료가 되지 않는다. embedded migration과 DB의 version, success,
  checksum 집합이 정확히 같아야 한다.
- migration SQL은 기존 배포 후 수정하지 않는다. 변경은 새 forward migration으로 추가한다.

## Runtime Connection

- API는 startup에서 PgPool 연결에 실패하면 즉시 종료한다.
- 모든 runtime connection은 `search_path=public, pg_catalog`, `TimeZone=UTC`, application name,
  statement/lock/idle-transaction timeout을 설정한다.
- graceful shutdown은 HTTP drain과 pool close를 하나의 절대 deadline 안에서 수행한다.
- availability와 lease 만료 판단은 PostgreSQL clock을 기준으로 한다. application node의 wall clock을
  authoritative time으로 사용하지 않는다.

## Tenant And Concurrency

- guild-owned aggregate는 `(guild_id, id)` 형태의 composite key/FK로 tenant 경계를 강제한다.
- 모든 composite FK는 child column을 선두로 하는 non-partial index를 가진다.
- 경쟁 command는 transaction 안에서 row lock, revision/state predicate, unique/check constraint로
  직렬화한다.
- deadlock SQLSTATE `40P01`은 같은 idempotency identity를 유지한 채 begin/operation/commit 전체를
  최대 3회 bounded backoff로 다시 실행한다. 다른 SQL 오류는 자동 재시도하지 않는다.
- idempotency row, domain mutation, audit row, outbox event는 같은 transaction에서 commit한다.
- 같은 idempotency command는 transaction advisory try-lock으로 직렬화하며, 이미 실행 중이면 DB row
  lock을 기다리지 않고 `in_progress`를 반환한다.
- outbox claim은 `FOR UPDATE SKIP LOCKED`와 owner별 UUIDv7 lease token을 사용한다. ack/nack은
  owner와 token이 모두 일치할 때만 반영하며 nack 재예약 시각은 DB clock과 retry delay로 계산한다.

## Roles And Recovery

- migration role과 runtime role의 credential을 분리한다.
- provisioning은 두 role 이름이 같으면 어떤 role/database도 만들기 전에 실패한다.
- runtime role은 schema 생성, table truncate, `_sqlx_migrations` 변경을 할 수 없다.
- audit/MMR/auction action·point ledger는 runtime에서도 append-only이며 UPDATE/DELETE를 금지한다.
- future table default privilege는 SELECT/INSERT만 열고, release grant transaction이 mutable table의
  UPDATE/DELETE를 명시적으로 연다.
- 배포 전 empty DB migration과 실제 PostgreSQL integration test를 실행한다.
- backup은 exported repeatable-read snapshot으로 dump하고 별도 빈 database에 restore한 뒤 exact
  migration signature와 snapshot 시점의 핵심 원장 row count를 비교한다.
