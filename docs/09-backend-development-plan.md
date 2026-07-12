# MonkeyBot Backend Development Plan

작성일: 2026-07-12

## 1. 문서의 지위

이 문서는 MonkeyBot 백엔드 구현의 상세 실행 기준이다.

판단 우선순위:

1. 현재 구현된 SvelteKit route, component 동작, props, command type, fixture 의미
2. 이 문서와 docs의 business workflow
3. mookbot_v3의 검증된 MMR 및 운영 로직

프론트와 기존 문서가 충돌하면 프론트를 따른다. 프론트에 없는 규칙은 docs를
따르고, docs에도 없는 세부는 이 문서의 보수적인 기본안을 따른다. API 연결
중에도 현재 route와 시각 구조는 유지한다.

## 2. 확정 기술

| 영역            | 결정                                                              |
| --------------- | ----------------------------------------------------------------- |
| Web             | SvelteKit frontend only                                           |
| Backend         | Rust + Axum                                                       |
| Database        | VPS 내부 PostgreSQL                                               |
| SQL             | SQLx                                                              |
| Realtime        | Axum WebSocket + Valkey Pub/Sub                                   |
| Source of truth | PostgreSQL only                                                   |
| Web auth        | Discord OAuth2 only                                               |
| Internal auth   | Bot service principal credential                                  |
| Deploy          | VPS + Docker Compose + nginx                                      |
| 순서            | Frontend contract -> Axum boundary -> 최소 Bot -> vertical slices |

폐기:

- SvelteKit fullstack backend
- Cloudflare D1/KV
- Drizzle backend
- Fastify/Node API
- DB와 Valkey의 원장 이중 저장

Valkey는 Pub/Sub, presence, rate limit, 재생성 가능한 cache만 담당한다.

## 3. Frontend 표준

### 3.1 Wire 규칙

| 항목              | 표준                              |
| ----------------- | --------------------------------- |
| ID                | opaque JSON string                |
| Discord snowflake | string                            |
| JSON              | camelCase                         |
| DB column         | snake_case                        |
| Time              | UTC RFC 3339                      |
| Lane              | TOP, JUNGLE, MID, BOTTOM, SUPPORT |
| Entry team        | TEAM_1, TEAM_2                    |
| Game side         | BLUE, RED                         |
| Auction format    | TEN_PERSON, TWENTY_PERSON         |
| Best of storage   | 1, 3, 5                           |
| MVP new match     | BO3                               |
| Conflict          | HTTP 409 + revision_conflict      |

상대 시각은 frontend가 만든다. BOTTOM의 UI label은 원딜로 표시할 수 있다.

### 3.2 Route와 aggregate

| Frontend route                                | Backend owner                 | WS   |
| --------------------------------------------- | ----------------------------- | ---- |
| /                                             | session default guild         | 없음 |
| /g/{guildId}                                  | operations projection         | 없음 |
| /g/{guildId}/series                           | recruitment/series projection | 없음 |
| /g/{guildId}/recruitments/{id}/entry          | entry draft                   | 사용 |
| /g/{guildId}/series/{id}/pick-ban             | series draft/game command     | 사용 |
| /g/{guildId}/series/{id}/result               | series ledger                 | 없음 |
| /g/{guildId}/auction                          | auction projection            | 없음 |
| /g/{guildId}/auction/recruitments/{id}/draft  | auction setup                 | 사용 |
| /g/{guildId}/auction/tournaments/{id}/draft   | auction tournament            | 사용 |
| /g/{guildId}/auction/tournaments/{id}/bracket | bracket/match                 | 사용 |
| /g/{guildId}/auction/tournaments/{id}/result  | tournament ledger/revert      | 없음 |
| /g/{guildId}/records                          | guild records                 | 없음 |
| /records                                      | Riot records                  | 없음 |
| /g/{guildId}/leaderboard                      | leaderboard                   | 없음 |
| /users/{discordId}                            | minimal profile               | 없음 |
| /me                                           | self settings                 | 없음 |
| /g/{guildId}/admin                            | guild admin                   | 없음 |
| /admin/status                                 | service status                | 없음 |

WS는 위 다섯 편집 workspace에만 사용한다.

### 3.3 API 연결 때 보완할 component

| Component                               | 보완                                                    |
| --------------------------------------- | ------------------------------------------------------- |
| EntryDraftBoard                         | controlled snapshot, reset/reopen/createSeries callback |
| ScrimPickBanResultInputBoard            | cell/submit/deleteLast/revert callback                  |
| SeriesResultLedgerBoard                 | nullable `durationSeconds`, `-` 표시 adapter            |
| AuctionTournamentCaptainPointSetupBoard | create tournament callback                              |
| AuctionMatchStartingLineupInputBoard    | match ID/revision, role/lock callback                   |
| AuctionTournamentOverviewBoard          | operator checkpoint revert callback                     |
| GuildMemberManagementPanel              | nullable `mainLane`, `미설정` adapter                   |
| OperationsQueueBoard                    | `RESULT` 작업을 pick-ban 입력 route로 연결              |
| Admin page handler                      | silent return 대신 typed 409                            |
| /records load                           | q를 Riot lookup에 전달                                  |

`apps/web/src/lib/contracts/dashboard.ts`의 lowercase kind/3단계 priority schema는 현재 어느 route도
사용하지 않는 초기 흔적이다. OpenAPI 근거로 사용하지 않고 실사용 `OperationsQueueItem` 계약으로
교체한 뒤 제거한다.

HTTP 성공 전 local 상태를 server 원장으로 확정하지 않는다.

## 4. MVP 경계

포함:

- Discord OAuth session과 multi-guild authorization
- 일반 entry -> series -> game -> MMR -> revert
- 경매 10인/20인 setup -> auction -> bracket -> lineup -> result -> MMR
- records, leaderboard, minimal profile
- Riot accounts, champion preferences, privacy, default guild
- guild members/settings/audit/revert/status
- 제한적 WS invalidate/presence
- Bot용 동일 application service/API

제외:

- 현재 게임/Riot Spectator
- CRDT/OT/offline mutation queue
- 범용 undo/redo
- Hard Fearless server 강제
- 웹 시즌 변경/일괄 등록/시즌 초기화/오래된 내전 정리
- KDA/CS를 custom match 결과의 필수값으로 사용
- 자체 회원가입과 Discord 외 OAuth

## 5. 목표 아키텍처

```text
Browser
  -> nginx
     -> SvelteKit web
     -> Axum /api, /oauth, /ws

Axum HTTP adapter
  -> application command/query
  -> domain policy
  -> SQLx repository
  -> PostgreSQL
  -> transactional outbox

Outbox relay
  -> Valkey Pub/Sub
  -> Axum WS hub
  -> invalidate
```

원칙:

- PostgreSQL이 상태와 revision의 유일한 원장이다.
- handler는 HTTP parsing/response만 담당한다.
- application service가 transaction과 use case를 소유한다.
- domain은 상태 전이와 invariant를 소유한다.
- repository는 guild scope와 SQL을 소유한다.
- 외부 HTTP 호출을 DB transaction 안에서 기다리지 않는다.
- Web과 Bot은 같은 command service에 도달한다.

## 6. Rust 구조

초기에는 modular monolith로 시작한다.

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
openapi/monkeybot.yaml
apps/api/
  Cargo.toml
  migrations/
  src/
    main.rs
    lib.rs
    bootstrap.rs
    config.rs
    telemetry.rs
    error.rs
    http/{middleware,routes}
    auth/
    application/{command,query}
    domain/
    repository/
    infrastructure/{postgres,valkey}
    integrations/{discord,riot,bot}
    realtime/
    jobs/
  tests/{contract,integration}
apps/web/src/lib/api/{generated,adapters}
```

domain/contracts/store crate 분리는 실제 공유 또는 compile boundary가 필요할
때만 한다.

Dependency 계열:

- Axum, Tokio, Tower, tower-http
- Serde, serde_json
- SQLx PostgreSQL/migrate/time/uuid/json
- tracing, tracing-subscriber, thiserror
- reqwest rustls, OAuth/cookie/session primitive
- Tokio 기반 Valkey-compatible client
- OpenAPI generator와 TypeScript codegen
- uuid, time, secrecy

구현 시작 시 stable/MSRV/advisory를 확인해 version을 pin하고 Cargo.lock을
커밋한다.

### 6.1 AppState

```text
Config
PgPool
Valkey connection manager
Discord client
Riot client
Session service
Realtime hub
Clock
ID generator
```

Clock/ID generator는 test double로 교체 가능하게 한다.

### 6.2 Middleware

1. trusted proxy/request ID
2. structured tracing
3. security headers
4. body size
5. request timeout
6. session extraction
7. CSRF/Origin
8. route rate limit
9. handler
10. error normalization

guild permission은 typed extractor 또는 application boundary에서 명시한다.

### 6.3 Graceful shutdown

1. readiness false와 새 accept 중단
2. in-flight HTTP drain
3. WS close
4. outbox lease 반환
5. pool close
6. drain timeout 뒤 종료

## 7. HTTP 계약

모든 guild resource는 /api/guilds/{guildId}/ 아래에 둔다.

```text
/api/me/**
/api/users/{discordId}/**
/api/riot/**
/api/guilds/{guildId}/operations
/api/guilds/{guildId}/recruitments/**
/api/guilds/{guildId}/series/**
/api/guilds/{guildId}/auction-recruitments/**
/api/guilds/{guildId}/auction-tournaments/**
/api/guilds/{guildId}/auction-matches/**
/api/guilds/{guildId}/records/**
/api/guilds/{guildId}/leaderboard
/api/guilds/{guildId}/members/**
/api/guilds/{guildId}/settings
/api/guilds/{guildId}/audit-logs
/api/guilds/{guildId}/reverts/**
```

전역 /api/series, /api/recruitments, /api/leaderboard는 사용하지 않는다.

Success:

```json
{
	"data": {
		"id": "0190f480-0000-7000-8000-000000000002",
		"revision": 43
	},
	"meta": {
		"requestId": "01J..."
	}
}
```

Conflict:

```json
{
	"error": {
		"code": "revision_conflict",
		"message": "최신 상태를 다시 확인해야 합니다.",
		"requestId": "01J...",
		"details": {
			"expectedRevision": 42,
			"currentRevision": 43,
			"updatedAt": "2026-07-11T11:20:08Z",
			"updatedBy": "954301284660912138"
		}
	}
}
```

| HTTP | 의미                                |
| ---- | ----------------------------------- |
| 400  | syntax/query 오류                   |
| 401  | session 없음/만료                   |
| 403  | 권한 없음                           |
| 404  | resource 없음/tenant 은폐           |
| 409  | revision/state/idempotency conflict |
| 422  | domain invariant                    |
| 429  | rate limit                          |
| 503  | temporary dependency failure        |

Mutation:

- 모든 편집 aggregate에 expectedRevision.
- 성공은 새 revision 또는 최신 snapshot.
- 409는 write/audit/outbox/revision 증가 없음.
- 위험 command는 autosave 금지.
- MMR/score/낙찰 결과는 server 계산.

Idempotency-Key 필수:

- series/tournament 생성
- 경매 tournament `/commands` 7종 전체와 tournament cancel
- game submit/void
- bracket/lineup lock
- series 조기 종료/취소
- admin MMR 수동 조정/되돌리기
- checkpoint apply
- season rotate
- Bot interaction. Discord interaction ID를 key로 사용

key, principal, route, aggregate, request hash, response를 PostgreSQL에 저장한다.
entry 이동, pick/ban cell, setup field처럼 revisioned draft 편집은 key를 강제하지 않는다.

### 7.1 OpenAPI

1. frontend fixture/control 의미 확인
2. Rust DTO 변경
3. OpenAPI snapshot diff
4. TypeScript 재생성
5. adapter compile
6. golden contract test

한국어 display label은 wire enum에 넣지 않는다.

## 8. 인증과 권한

Session:

- Discord OAuth2 only
- state, PKCE, redirect allowlist
- random opaque cookie
- DB에는 token hash/expiry/revocation
- login rotation과 logout revoke

Cookie:

```text
monkeybot_session
Secure
HttpOnly
SameSite=Lax
Path=/
host-only
```

production은 same-origin이며 wildcard CORS를 열지 않는다. Unsafe method는
Origin과 CSRF header를 확인한다.

Role:

```text
MEMBER
OPERATOR
SERVICE_ADMIN
```

- read는 membership 또는 공개 정책.
- operation mutation은 OPERATOR.
- captain 별도 mutation 권한은 MVP 제외.
- /admin/status는 SERVICE_ADMIN ID allowlist.
- 다른 tenant resource는 404.

Discord membership/role freshness는 Phase 3 spike에서 OAuth member scope, Bot
REST, Bot event sync를 비교한다. 오래된 cache만으로 mutation을 허용하지 않는다.

Privacy:

- external read는 guild isExternallyPublic와 user privacy를 함께 확인.
- 같은 guild의 game/MMR 원장은 personal privacy로 숨기지 않음.
- operation draft/admin/audit/permission은 외부 공개 금지.

## 9. PostgreSQL 모델

### 9.1 타입

| 값                | DB                       |
| ----------------- | ------------------------ |
| Discord snowflake | text                     |
| 신규 domain ID    | UUIDv7, API string       |
| legacy ID         | legacy_id/import mapping |
| Time              | timestamptz              |
| Revision          | bigint, 0 이상           |
| MMR               | double precision         |
| Point/count       | integer                  |
| Status/lane       | text + CHECK             |
| Audit/checkpoint  | versioned jsonb          |

PostgreSQL enum과 aggregate JSONB blob은 초기 기본값으로 사용하지 않는다. MMR은 v3 ELO의
소수 누적 결과를 그대로 보존하고 UI에서만 반올림한다. Table별 column, PK/FK, partial
unique, delete policy의 canonical 정의는 [PostgreSQL Schema Plan](05-db-schema.md)을 따른다.

### 9.2 Table group

```text
Identity
  users, oauth_login_attempts, user_sessions
  riot_accounts, user_privacy_settings, user_guild_preferences
  user_champion_preferences

Guild
  guilds, guild_members, guild_member_discord_roles
  guild_config, guild_admin_state, seasons, guild_counters

General
  recruitments, recruitment_participants, recruitment_preferred_lanes
  recruitment_entry_drafts, recruitment_entry_assignments
  series, series_participants

Game/MMR
  game_inputs, game_input_picks, game_input_bans
  games, game_participants, game_participant_stats, game_bans
  mmr_ledger_entries, user_lane_mmr

Auction
  auction_recruitments, auction_recruitment_participants
  auction_setup_drafts, auction_setup_teams
  auction_tournaments, auction_teams, auction_team_members
  auction_bid_candidates, auction_lots, auction_actions, auction_point_ledger
  auction_bracket_slots, auction_matches, auction_match_lineups

Safety
  revert_checkpoints, admin_audit_log
  idempotency_records, outbox_events, bot_notification_outbox, bot_instances
```

Custom match KDA/CS/damage는 필수 column이 아니다. Riot external match는 별도
cache/projection이다.

### 9.3 Identity와 tenant

```text
series.source_recruitment_id non-cancelled partial UNIQUE FK
auction_tournaments.source_recruitment_id non-cancelled partial UNIQUE FK
```

동일-ID legacy rule은 폐기한다.

- 모든 child에 guild_id.
- parent에 (guild_id, id) unique.
- child에 (guild_id, parent_id) composite FK.
- repository method는 GuildId를 필수로 받음.
- cross-guild 표 기반 test 필수.

MVP에서는 복잡한 RLS를 부분 적용하지 않는다.

### 9.4 Constraint

- entry participant당 한 slot, slot당 한 participant.
- game은 series_id XOR auction_match_id.
- confirmed game은 parent/game number partial unique이며 void 후 재입력을 허용한다.
- game participant는 game/team/lane과 game/user unique. side는 game sidePreset에서 파생한다.
- game ban은 game/side/slot unique, slot 범위 1..5다. 입력된 ban 수가 5개일 필요는 없다.
- Riot account가 있는 user는 main이 정확히 하나다.
- auction participant는 tournament에서 한 team.
- auction team은 active roster slot 1..5만 사용한다.
- team points는 0 이상.
- tournament active lot 최대 하나.
- tournament active candidate 최대 하나.
- bracket team은 한 slot에만 배정된다.
- lineup은 match/team/lane 및 match/user unique.
- lineup user는 해당 team member.
- MMR game result는 game/user/lane 중복 금지, reversal은 원본 entry당 한 번이다.

### 9.5 Index

- guild/status/updated_at recruitment/series/auction.
- non-cancelled source_recruitment_id partial unique.
- game parent/game_number.
- guild/user/season/lane MMR unique.
- guild/season/lane ranking.
- guild/user game history.
- tournament status/updated_at.
- unpublished outbox/available_at.
- active session token_hash.
- Riot PUUID/normalized Riot ID.
- deleted_at IS NULL partial index.

EXPLAIN ANALYZE로 실제 query를 검증한 index만 유지한다.

## 10. 상태와 transaction

```text
Recruitment
  OPEN -> CLOSED -> CONVERTED
  OPEN/CLOSED -> CANCELLED
  CLOSED/CANCELLED -> OPEN은 explicit reopen

Series
  IN_PROGRESS -> COMPLETED/CANCELLED
  COMPLETED terminalReason = SCORE_REACHED | EARLY_WIN | EARLY_DRAW
  생성 revert: game 전 IN_PROGRESS -> CANCELLED, source recruitment -> CLOSED

Auction recruitment
  OPEN -> CLOSED -> CONVERTED
  OPEN/CLOSED -> CANCELLED

Auction tournament
  BIDDING -> BRACKET_SETUP -> IN_GAME -> COMPLETED
  완료 전 단계 -> CANCELLED
  생성 revert: 첫 bidding action 전 BIDDING -> CANCELLED, source recruitment -> CLOSED
  bracket 생성 revert: game 전 IN_GAME -> BRACKET_SETUP, 생성 match -> CANCELLED

Auction match
  LINEUP_PENDING -> READY -> IN_PROGRESS -> COMPLETED
  READY -> LINEUP_PENDING은 game 전 explicit revert
```

팀장/시작 point 설정은 tournament 생성 전 `auction_setup_drafts` aggregate에서 완료한다.

공통 mutation:

```text
session/permission/schema
-> transaction
-> aggregate SELECT FOR UPDATE
-> expectedRevision
-> invariant
-> state/ledger/checkpoint/audit
-> 허용된 WS topic이 있는 aggregate만 outbox
-> revision + 1
-> commit
-> response
```

기본 isolation은 READ COMMITTED + 좁은 row lock이다.
생성 전 일반 전환은 `recruitment -> entry draft -> series INSERT`, 경매 전환은
`auction recruitment -> setup draft/team_index order -> tournament INSERT` 순서다. source 모집과
생성된 child를 함께 바꾸는 lifecycle command도 항상 source를 먼저 잠근다. 생성 이후 경매
mutation은 `tournament -> affected match UUID order -> team/child -> MMR`로 고정한다. match
endpoint도 tenant read로 tournament ID를 resolve한 뒤 이 순서를 지킨다. 경매 game command의
client token은 match `expectedRevision` 하나이며, locked tournament는 현재 상태를 재검증하고 서버가
revision을 올린다. 기존 tournament를 먼저 잠근 뒤 source recruitment를 뒤늦게 잠그는 구현은
허용하지 않는다.

### 10.1 Entry

MOVE_PARTICIPANT는 frontend swap 규칙을 유지한다.

- 빈 slot: 이동.
- 기존 slot이 있는 사용자가 점유 slot으로 이동: 교환.
- 후보가 점유 slot으로 이동: 기존 사용자는 후보.
- recruitment participant가 아니면 거부.

전체 assignments replace endpoint는 만들지 않는다.

### 10.2 Game과 MMR

1. aggregate/revision lock.
2. bestOf와 next setNumber.
3. winner 필수, picks/bans optional.
4. game/participant/ban insert.
5. v3 호환 MMR pure function.
6. user/lane 정렬 순으로 `INSERT ... ON CONFLICT DO NOTHING`해 missing MMR row 생성.
7. 같은 순서로 MMR row lock.
8. mmr_ledger_entries append/summary update.
9. score/completion.
10. checkpoint/audit/outbox/revision.
11. commit.

v3 MMR input/output을 golden test로 고정한 뒤 Rust로 이식한다.

Game revert는 game void, reversal MMR ledger, summary/score/status 복구를 한
transaction에서 처리한다. 대상 game entry가 모든 참가자의 해당 season/lane ledger tail일
때만 허용하며 이후 변경이 있으면 dependency conflict로 거부한다.

### 10.3 Auction

Bid finalize:

1. tournament/active lot lock.
2. revision/status.
3. member/team lock.
4. 미배치/정원/bid 검증.
5. point 차감.
6. AWARD_BID action append, point ledger 기록, team member 배치.
7. lot 종료.
8. checkpoint/audit/outbox/revision.
9. commit.

동시 finalize 중 하나만 성공한다.

Auction invariant:

- TEN_PERSON: 2팀 결승.
- TWENTY_PERSON: 4팀 준결승 A/B와 결승.
- 신규 match는 BO3. 저장된 legacy BO1/BO3와 향후 BO5는 조회/score 계산에서 보존.
- 양 팀 TOP/JUNGLE/MID/BOTTOM/SUPPORT 각 한 명.
- lineup lock 전 result 거부.
- 경매도 guild MMR 반영.

### 10.4 Admin

MMR 수동 조정은 MANUAL_ADJUSTMENT ledger와 actor/note/before/after를 남긴다.
웹 settings mutation은 displayName, operatorRoleId, recruitmentChannelId,
logChannelId, isExternallyPublic만 허용한다. seasonId는 read-only다.

## 11. Audit, checkpoint, outbox

Audit는 append-only다.

```text
guild_id, principal_type, principal_id
action, target_type, target_id
request_id, before_summary, after_summary
result, note, created_at
```

Checkpoint:

```text
target_type, target_id, target_revision
schema_version, restore_payload, status
created_by/at, reverted_by/at
```

checkpoint 이후 종속 결과가 있으면 unsafe revert를 거부한다.

Transactional outbox:

```text
domain transaction
  -> outbox insert
  -> commit

relay
  -> 짧은 transaction에서 SELECT FOR UPDATE SKIP LOCKED
  -> claim별 lease_token과 lease_owner/lease_until 기록 후 commit
  -> transaction 밖에서 Valkey publish
  -> owner+token이 모두 일치할 때만 성공/실패 결과 기록
```

전달은 at-least-once이며 eventId/revision으로 중복을 제거한다. Valkey 장애에도
HTTP 원장은 commit되고 backlog는 복구 후 전달된다.

Discord notification outbox는 delivered와 terminal failed를 구분한다. 429/5xx/timeout만 capped
backoff로 재시도하고 non-retryable 4xx 또는 최대 시도 초과는 `failed_at/failure_code`를 기록해
pending claim에서 제외한다. create message는 stable nonce와 Discord nonce uniqueness를 사용하고,
timeout-after-send를 dedup window 안에서 확인하지 못하면 `DELIVERY_UNKNOWN`으로 보내 blind retry로
중복 메시지를 만들지 않는다. `attempts`는 claim 횟수이며 claim transaction에서 증가하고 최초
claim 시각도 함께 기록한다. nack에서는 다시 증가시키지 않으며 마지막 claim 실패 또는 마지막
claim의 lease 만료는 `MAX_ATTEMPTS` terminal 상태로 정리한다.

## 12. WebSocket과 Valkey

Endpoint는 /ws 하나다.

```text
guild:{guildId}:recruitment:{id}
guild:{guildId}:series:{id}
guild:{guildId}:auction-recruitment:{id}
guild:{guildId}:auction-tournament:{id}
guild:{guildId}:auction-match:{id}
```

Event:

```json
{
	"type": "invalidate",
	"eventId": "0190f4a2-8b3c-7d5e-8f01-23456789abcd",
	"topic": "guild:123:auction-tournament:0190f49d-3e20-7a41-9810-123456789abc",
	"resourceId": "0190f49d-3e20-7a41-9810-123456789abc",
	"revision": 43,
	"occurredAt": "2026-07-11T11:20:12Z"
}
```

- subscribe마다 auth.
- full state/mutation payload 금지.
- bounded queue/frame/subscription.
- slow client disconnect.
- ping/pong/idle timeout.
- reconnect backoff + HTTP refetch.
- duplicate/out-of-order 허용.

Frontend state:

```text
connection = connected | reconnecting | offline
save       = saved | saving | dirty | conflict
```

Valkey key:

```text
monkeybot:{env}:pubsub:{topic}
monkeybot:{env}:presence:{topic}:{connectionId}
monkeybot:{env}:cache:riot:{key}
monkeybot:{env}:ratelimit:{principal}:{route}
```

Draft, bid, point, lineup, game, MMR은 Valkey에 저장하지 않는다.

## 13. External integration

Discord:

- connect/total timeout.
- 제한 retry와 Retry-After.
- membership freshness metric.
- 운영 mutation 전에 정책상 최신 role 확인.

Riot:

- PUUID가 canonical account key.
- Account-v1로 GameName#TagLine 확인.
- region/platform/icon/lastSyncedAt.
- account가 있으면 main 하나.
- external call은 DB transaction 밖.
- rate limit/timeout/stale cache 구분.
- guild MMR과 결합 금지.
- Spectator 제외.

Bot:

- DB 직접 접근 금지.
- service principal 분리.
- private network + rotating credential.
- Discord interaction ID idempotency.
- Web/Bot 동일 command.
- notification outbox와 heartbeat.

## 14. 성능, 보안, 관측성

성능 원칙:

- async handler에서 blocking I/O 금지.
- DB pool wait 측정.
- projection N+1 금지.
- cursor pagination.
- records summary/detail 분리 가능.
- bounded outbound concurrency/timeout.

Provisional regression gate:

| 항목                 | 초기 목표                  |
| -------------------- | -------------------------- |
| internal read p95    | 200ms 이하                 |
| normal mutation p95  | 300ms 이하                 |
| invalidate p95       | commit 후 1초 이하         |
| non-409 server error | 0.5% 미만                  |
| WS smoke             | 500 connections            |
| API smoke            | 100 read + 20 mutation RPS |

실제 VPS baseline에 따라 근거와 함께 조정한다.

초기 timeout:

- API 10s.
- DB statement 3s.
- Discord/Riot connect 3s, total 10s.
- graceful drain 20s.
- WS ping/idle 30s/90s.

Structured log:

```text
request_id, trace_id, route, method, status, latency
guild_id, principal_type, user_id, aggregate_id, revision, error_code
```

Token/cookie/API key/전체 draft는 log에 남기지 않는다.

Metric:

```text
http_requests_total
http_request_duration_seconds
db_pool_wait_seconds
db_transaction_duration_seconds
revision_conflicts_total
idempotency_replays_total
ws_connections
ws_subscribe_denied_total
ws_publish_failures_total
outbox_backlog
notification_terminal_failures_total
riot_requests_total
discord_requests_total
```

Health:

- /healthz: process only.
- /readyz: PostgreSQL과 migration version.
- Valkey down: degraded, HTTP 원장은 ready.
- /api/admin/status: API/PostgreSQL/Bot/Valkey.
- /metrics: VPS internal only.

Security:

- non-root container.
- SQLx bind only.
- body/frame limit.
- secret file/env.
- production stack/SQL 숨김.
- release dependency audit.

## 15. 테스트 전략

Domain unit:

- v3 MMR golden vector.
- entry swap/state transition.
- auction draw/award/unsold/manual.
- bracket/lineup invariant.
- privacy policy.

Contract:

- OpenAPI snapshot.
- generated TypeScript compile.
- camelCase/error/revision/WS schema.
- frontend 의미를 옮긴 golden response.

PostgreSQL:

- empty migration.
- composite FK tenant isolation.
- soft delete.
- idempotency replay.
- outbox claim/retry.
- game MMR participant/opponent FK.
- typed reversal과 self-reference/action 재사용 거부.

Concurrency:

- same revision one success.
- concurrent bid finalize one commit.
- duplicate game number prevention.
- deterministic MMR lock order.
- lineup lock/result race.
- revert/next game race.

Auth/HTTP:

- OAuth state/PKCE/redirect.
- session rotation/expiry/logout.
- CSRF/Origin.
- cross-guild 404.
- role matrix/body/rate limit.

WS/failure:

- handshake/subscribe auth.
- duplicate/out-of-order/reconnect.
- slow client/session expiry.
- Valkey restart/outbox replay.
- transaction rollback atomicity.
- timeout-after-commit idempotency.
- PostgreSQL reconnect.
- Riot/Discord timeout/429/malformed.
- SIGTERM drain.

Validation:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm check
pnpm lint
pnpm test
pnpm build
```

Integration test는 실제 PostgreSQL/Valkey container를 사용한다.
`query!`/`query_as!`/`query_scalar!` 계열 compile-time macro를 도입하면 migration이 적용된 임시
PostgreSQL을 `DATABASE_URL`로 지정하고 `.sqlx` metadata commit과
`cargo sqlx prepare --workspace --check -- --all-targets --all-features`를 이 gate에 추가한다. 동적
SQLx API만 사용하는 단계에서는 빈 `.sqlx` metadata를 만들지 않는다.

## 16. Legacy migration

mookbot_v3는 in-place 수정하지 않는다.

1. v3 runtime config에서 draft provider가 Valkey인지 D1인지 확인.
2. rehearsal은 D1과 authoritative draft store를 read-only export.
3. 실제 cutover는 v3 mutation write freeze 후 D1과 Valkey `entry:*`/`pickban:*`를 다시 full export.
4. D1 `auction-lineup:*`과 두 source artifact checksum을 manifest에 고정.
5. PostgreSQL staging에 원본을 보관하고 legacy support schema/run을 생성.
6. explicit guild/user/ID/season/Riot normalization mapping.
7. aggregate 단위로 변환하고 child 오류는 parent unit 전체를 quarantine.
8. integer timestamp -> timestamptz, mandatory target field/actor/lot/input을 결정적으로 합성.
9. cache와 범용 guild_kv는 폐기/재생성.
10. count/orphan/duplicate/tenant crossing/MMR/point 합계와 active quarantine=0 검증.
11. shadow read comparison과 accepted omission/reentry 승인.

과거 경매 MMR은 자동 소급하지 않는다.

Migration 원칙:

- SQLx versioned forward migration.
- API startup auto migration 금지.
- one-shot migration command.
- expand -> backfill -> switch -> contract.
- backup/restore rehearsal 선행.
- down보다 verified restore/forward fix 우선.

## 17. 배포 기준

| Service       | Host port       |
| ------------- | --------------- |
| PostgreSQL    | 127.0.0.1:10010 |
| frontend main | 20010           |
| backend main  | 20020           |
| frontend dev  | 30010           |
| backend dev   | 30020           |

- PostgreSQL public 노출 금지.
- main/dev 별도 DB/role/secret.
- Valkey namespace 분리.
- nginx /api, /oauth, /ws -> Axum.
- 기존 mookbot_v3 site/upstream 변경 금지.
- backend dev부터 배포.

Deploy order:

1. tree/image tag.
2. PostgreSQL backup.
3. migration dry run/one-shot.
4. image pull/start.
5. healthz/readyz.
6. auth/read smoke.
7. mutation/idempotency smoke.
8. WS upgrade/invalidate smoke.
9. metrics/outbox backlog.

Backup 초기안:

- nightly pg_dump custom.
- daily 7, weekly 4, monthly 6.
- VPS 외부 복제.
- 분기별 restore rehearsal.
- 데이터 증가 후 WAL/PITR 결정.

## 18. 단계별 실행 계획

각 Phase는 exit criteria를 통과한 뒤 다음으로 간다.

### Phase 0. Contract freeze

목표: mock UI의 server-owned 동작을 확정한다.

Task:

- BE-000 route/component/control inventory.
- BE-001 fixture -> golden JSON.
- BE-002 enum/ID/time/error.
- BE-002A root DTO `id`/`displayNumber` 분리와 header/deep-link adapter.
- BE-003 guild-scoped endpoint matrix.
- BE-004 WS schema.
- BE-005 문서 충돌 제거.
- BE-006 local-only component wiring backlog.
- BE-007 frontend Drizzle/D1 placeholder import와 제거 시점 확인.
- FE-008 admin member `mainLane: Lane | null` DTO와 `미설정` 표시 adapter.
- FE-009 미사용 `lib/contracts/dashboard.ts` 폐기와 실사용 operations DTO 고정.
- FE-010 `RESULT` actionable href를 `/series/{id}/pick-ban`으로 교정.

Artifact:

- OpenAPI skeleton.
- route matrix.
- WS JSON schema.
- request/response fixtures.
- decision log.

Test:

- 모든 button을 local/query/command로 분류.
- 모든 expand detail을 read DTO에 매핑.
- 미정 항목에 owner와 decision Phase 지정.

Exit:

- API adapter 전환에 UI 재설계가 필요 없다.
- frontend와 docs의 알려진 핵심 충돌이 없다.

### Phase 1. Axum foundation

Task:

- BE-100 Cargo workspace/API crate.
- BE-101 typed config/startup validation.
- BE-102 router/AppState/bootstrap.
- BE-103 request ID/error envelope.
- BE-104 tracing/redaction.
- BE-105 timeout/body/security middleware.
- BE-106 healthz/readyz.
- BE-107 graceful shutdown.
- BE-108 Docker multi-stage non-root build.

Test:

- invalid env startup failure.
- panic/internal error normalization.
- health secret redaction.
- SIGTERM drain.

Exit:

- fmt/clippy/test pass.
- local/container config parity.
- liveness/readiness separation.

### Phase 2. PostgreSQL foundation

Migration:

```text
0001_bootstrap
0002_identity_auth_riot
0003_guild_members_seasons_config
0004_recruitments_entry
0005_series_core
0006_auction_core_bracket_matches
0007_game_inputs_games_mmr
0008_audit_checkpoint_idempotency_outbox
0009_read_indexes
```

Task:

- BE-200 PgPool/statement timeout.
- BE-201 SQLx migration.
- BE-202 runtime/migration role.
- BE-203 typed ID/lane/status.
- BE-204 transaction helper.
- BE-205 tenant repository convention.
- BE-206 idempotency repository.
- BE-207 outbox relay skeleton.
- BE-208 test DB harness.
- BE-209 backup/restore smoke.

Test:

- empty DB migration.
- version mismatch readiness failure.
- cross-guild FK failure.
- unique/check/partial index.
- concurrent idempotency in-progress/replay/conflict.
- concurrent outbox claim.
- expired/reclaimed lease의 stale token ack가 새 claim을 갱신하지 못함.
- concurrent first MMR-row initialization.
- deadlock SQLSTATE bounded full-transaction retry.

Exit:

- actual PostgreSQL integration pass.
- dev/prod DB role procedure.
- restored DB readiness/count pass.

### Phase 3. Discord auth/guild context

Task:

- BE-300 membership freshness spike.
- BE-301 OAuth start/state/PKCE.
- BE-302 callback/user/session rotation.
- BE-303 logout/revoke.
- BE-304 CSRF/Origin.
- BE-305 GET /api/me.
- BE-306 GET /api/me/guilds.
- BE-307 PUT /api/me/default-guild.
- BE-308 GET /api/guilds/{guildId}.
- BE-309 Member/Operator/ServiceAdmin guard.
- BE-310 Bot service principal 인증 기반.

Test:

- redirect allowlist.
- expired/revoked session.
- logout cookie/DB revoke.
- role matrix.
- browser principal과 Bot service principal 분리.
- cross-guild 404.
- service-admin allowlist.

Exit:

- Discord test app login/logout.
- root redirect DTO.
- reusable typed principal/guild context.

### Phase 3.5. Minimal Bot operating path

이 단계는 테스트용 seed를 운영 데이터처럼 유지하지 않고 실제 Discord
interaction으로 이후 vertical slice의 입력 상태를 만들기 위한 최소 경로다.

Task:

- BE-350 길드 setup upsert와 운영자 역할/모집 채널 설정.
- BE-351 일반 모집 create/message bind/join/leave/preferred-lane/close command.
- BE-352 Discord interaction ID 기반 idempotency와 Bot service principal guard.
- BOT-350 Node 22 + discord.js runtime, health/readiness, command registration.
- BOT-351 `/초기설정` command.
- BOT-352 `/내전모집`과 참가/취소/라인 선택/엔트리 시작 component.

Test:

- setup/create interaction replay와 payload conflict.
- self action, operator action, unauthorized operator action.
- 10명 정원, 탈퇴 후 대체 참가, closed recruitment rejection.
- cross-guild 404와 Bot credential rejection.
- Bot renderer/custom ID/API contract.

Exit:

- 실제 test guild에서 setup과 일반 모집 snapshot을 생성할 수 있다.
- Bot은 Axum API만 사용하고 PostgreSQL에 직접 접근하지 않는다.
- Bot token 없이 API integration test와 Bot unit test를 실행할 수 있다.

### Phase 4. Read vertical slice

Task:

- BE-400 operations projection.
- BE-401 actionable recruitment.
- BE-402 active/completed series.
- BE-403 active/completed auction.
- BE-404 minimal profile.
- FE-400 route API adapter.
- FE-401 operations `RESULT` href/input route contract.

Test:

- empty/actionable/active/completed.
- 10/20 person rows.
- updatedAt ordering.
- tenant isolation.
- query count/N+1.

Exit:

- dashboard/series/auction no fixture.
- current row structure preserved.

### Phase 5. General scrim vertical slice

Task:

- BE-500 recruitment snapshot.
- BE-501 MOVE_PARTICIPANT/CLEAR.
- BE-502 reset/reopen.
- BE-503 separate series create.
- BE-504 pickban cell operation.
- BE-505 game submit.
- BE-506 v3 MMR port.
- BE-507 last game revert.
- BE-508 series ledger.
- BE-510 일반 모집/series notification outbox와 internal claim/ack/nack.
- FE-500 EntryDraft controlled adapter와 reset/reopen/createSeries control. 신규 series는 BO3.
- FE-501 PickBan controlled adapter와 cell/submit/deleteLast/revert control.
- FE-502 completed series outcome adapter including EARLY_DRAW.
- FE-503 result ledger nullable duration adapter.
- BOT-501 series created/completed notification과 web deep link.

Test:

- entry swap/candidate.
- no full JSON replace.
- 신규 BO3 score/completion과 imported BO1/BO3 read.
- optional pick/ban, required winner.
- operator early win/draw/cancel preserves existing game/MMR and renders nullable winner safely.
- failed transaction leaves game/MMR/revision unchanged.
- 같은 신규 user/lane을 포함한 동시 game submit은 PK race 없이 순차 ledger를 만든다.
- MMR laneRevision tail과 summary revision이 일치하고 reversal source type/inverse delta를 검증한다.
- revert restores MMR/score.

Exit:

- entry부터 completed result까지 PostgreSQL.
- MMR before/after와 ledger 합계 일치.
- Bot 모집에서 entry, series 완료 알림까지 같은 Axum command 경계를 사용.

### Phase 6. Limited realtime

Task:

- BE-600 WS protocol.
- BE-601 topic authorization.
- BE-602 outbox -> Valkey.
- BE-603 Valkey -> local hub.
- BE-604 presence TTL.
- BE-605 queue/frame/heartbeat.
- FE-600 connection/save state adapter.
- FE-601 invalidate/refetch/conflict.
- FE-602 focus/online/reconnect.

Test:

- two-client entry/pickban.
- same revision one success.
- dirty local invalidate.
- Valkey down/up replay.
- unauthorized topic.
- duplicate/out-of-order.
- slow client.

Exit:

- WS 없이도 HTTP save 정상.
- reconnect snapshot recovery.
- real RealtimeEditStateBar state.

### Phase 7. Auction vertical slice

Task:

- BE-700 auction recruitment snapshot.
- BE-701 captain/point + tournament create.
- BE-702 DRAW/CANCEL.
- BE-703 UNSOLD/RESTORE.
- BE-704 AWARD_BID.
- BE-705 current/unsold manual assign.
- BE-706 REVERT_LAST.
- BE-707 2/4 team bracket.
- BE-708 lineup role/lock.
- BE-709 auction game/MMR/revert.
- BE-710 tournament ledger.
- FE-700 captain setup callback.
- FE-701 auction command adapter.
- FE-702 selected match lineup.
- FE-703 result revert.

Test:

- roster/point/team invariant.
- mismatched lot-target/action-team/member/point FK rejection.
- action/point self-reversal과 acquisition action 재사용 rejection.
- pointRevision tail과 team currentPoints 일치.
- concurrent finalize one commit.
- timeout idempotency replay.
- 2-team final.
- 4-team semifinal/final.
- incomplete/duplicate lineup rejection.
- locked-before-result.
- auction MMR.
- tournament/match conflict.

Exit:

- TEN_PERSON/TWENTY_PERSON end-to-end.
- bid/score/MMR/revert transaction/audit.

### Phase 8. Records/self/Riot

Task:

- BE-800 records search summary.
- BE-801 general series detail.
- BE-802 10/20 auction grouped detail.
- BE-803 leaderboard.
- BE-804 champion preferences.
- BE-805 Riot account CRUD/main/refresh.
- BE-806 privacy.
- BE-807 external Riot records/cache.
- BE-808 default guild.

Test:

- q/season/type/lane/cursor.
- general/auction MMR before/after.
- 20-person tournament grouping.
- privacy/public matrix.
- main Riot uniqueness.
- Riot 429/timeout/stale.
- query performance.

Exit:

- current records expand structure reproduced.
- internal/external records separated.
- Riot failure does not affect internal records.

### Phase 9. Guild admin

Task:

- BE-900 member list.
- BE-901 manual MMR ledger.
- BE-902 member status.
- BE-903 typed settings.
- BE-904 checkpoint/revert.
- BE-905 audit log.
- BE-906 service status.
- FE-900 typed admin adapter.

Exclude:

- web bulk register.
- web season mutation/reset.
- generic maintenance dashboard.

Test:

- audit in same transaction.
- seasonId mutation rejection.
- expectedRevision 409.
- dependent revert rejection.
- service-admin status auth.

Exit:

- current four admin tabs no fixture.
- command boundary reusable by Bot.

### Phase 10. Remaining Bot boundary

Task:

- BE-1000 Bot client를 경매/사용자 조회/admin command로 확장.
- BE-1001 generic notification type과 retry 운영 확장.
- BE-1002 heartbeat.
- BE-1003 Bot-only season rotate command mapping.
- BOT-1000 auction recruitment/operation.
- BOT-1001 user lookup/admin command mapping.

Test:

- Web/Bot same command result.
- 일반/경매 duplicate Discord interaction.
- notification retry.
- non-retryable Discord 4xx와 max-attempt notification은 terminal failed가 된다.
- 마지막 claim 뒤 lease가 만료돼도 reaper가 terminal 처리한다.
- timeout-after-send는 같은 enforced nonce로 deduplicate되거나 DELIVERY_UNKNOWN으로 멈춘다.
- invalid service credential.

Exit:

- Bot has no direct DB access.
- 일반 모집 기반을 재사용하고 경매/admin flow도 API만 사용.

### Phase 11. Migration rehearsal

Task:

- BE-1100 D1/Valkey read-only export와 cutover write-freeze manifest.
- BE-1101 guild/season/Riot/legacy ID mapping.
- BE-1102 apply `migrations/legacy/0001_import_support.sql` and staging import.
- BE-1103 authoritative entry/pickban 및 D1 auction-lineup draft import.
- BE-1103A synthetic submitted game inputs and snapshot-origin mapping.
- BE-1103B aggregate-unit quarantine/remediation/accepted reentry workflow.
- BE-1104 count/MMR/orphan/point/partial-aggregate verifier.
- BE-1105 shadow comparison.
- BE-1106 cutover runbook.
- BE-1107 restore rehearsal.

Exit:

- no source v3 write.
- validation report.
- active aggregate unresolved quarantine 0.
- accepted omission/reentry operator sign-off.
- measured restore time.
- historic auction MMR decision recorded.

### Phase 12. Hardening/dev deploy

Task:

- BE-1200 load baseline.
- BE-1201 EXPLAIN/index review.
- BE-1202 fault injection.
- BE-1203 dependency/security audit.
- BE-1204 metric/runbook.
- BE-1205 backup automation.
- BE-1206 dev 30020 deploy.
- BE-1207 smoke/rollback.

Exit:

- performance gate or documented adjustment.
- HTTP ledger survives Valkey outage.
- PostgreSQL restore/SIGTERM/rollback pass.
- existing mookbot_v3 healthy.

Main backend deploy requires separate approval.

## 19. 첫 구현 순서

1. Phase 0 route matrix/golden JSON.
2. Axum health/error/config.
3. PostgreSQL migration harness.
4. session/guild context와 Bot service principal.
5. 최소 Bot `/초기설정`과 일반 모집 command 경계.
6. 실제 Discord interaction으로 생성한 상태의 operations/read 연결.
7. general entry.
8. game/MMR/revert와 series notification.
9. WS invalidate.
10. auction 10 person.
11. auction 20 person.

빈 endpoint scaffold를 전부 만들지 않는다. 각 vertical slice가 실제 frontend
route 하나를 fixture에서 PostgreSQL로 전환해야 한다.

## 20. Definition of Done

- frontend 동작과 DTO 의미 일치.
- guild tenant/permission 검증.
- OpenAPI/generated TS 갱신.
- success/error/revision contract test.
- PostgreSQL integration/concurrency test.
- audit/checkpoint/outbox 적용.
- structured log/metric.
- frontend check/lint/test/build.
- route/schema docs 갱신.
- secret/PII log 없음.
- frontend dev server를 새로 시작하지 않음.

## 21. Decision gate

| 결정                   | 시점        | 기본안                         |
| ---------------------- | ----------- | ------------------------------ |
| Discord role freshness | Phase 3     | Bot/Discord REST + short cache |
| OAuth token storage    | Phase 3     | 필요할 때만 encrypted          |
| OpenAPI tool           | Phase 1     | Axum/codegen spike             |
| Valkey client          | Phase 1     | Tokio/reconnect 검증           |
| New ID                 | Phase 2     | UUIDv7                         |
| Historic auction MMR   | Phase 11    | no automatic backfill          |
| WAL/PITR               | data growth | pg_dump/external copy first    |

Framework, DB, tenant, auction MMR, WS role은 더 이상 미정이 아니다.
