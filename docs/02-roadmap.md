# MonkeyBot Product Roadmap

작성일: 2026-07-11

## 방향

MonkeyBot은 mookbot_v3를 복사하는 프로젝트가 아니다. 현재 SvelteKit
프론트엔드에서 확정한 운영 흐름을 기준으로 Rust/Axum API와 PostgreSQL
원장을 새로 만들고, v3의 MMR과 검증된 운영 규칙만 선별 이식한다.

우선순위:

1. 현재 프론트 route와 control을 API 계약으로 고정한다.
2. PostgreSQL의 tenant, transaction, audit, revert 기반을 만든다.
3. 최소 Bot으로 실제 길드 설정과 일반 모집 데이터를 생성한다.
4. 일반 내전과 경매 내전을 vertical slice로 연결한다.
5. 동시 편집 route에만 WebSocket을 붙인다.
6. 조회/self/admin과 나머지 Bot 기능을 연결한다.
7. v3를 건드리지 않는 migration rehearsal 뒤 운영 전환을 판단한다.

상세 task와 exit criteria는 [Backend Development Plan](09-backend-development-plan.md)을
따른다.

## 목표 구조

```text
MonkeyScrimManager/
  apps/
    web/          SvelteKit frontend only
    api/          Rust + Axum modular monolith
    bot/          Discord interaction/notification
  openapi/        Axum API snapshot
  docs/
  deploy/
  scripts/
```

Axum 내부 경계:

```text
HTTP/WS adapter
  -> application command/query
  -> domain policy
  -> SQLx repository
  -> PostgreSQL
```

Valkey는 Pub/Sub, presence, rate limit, cache만 담당한다. draft, bid, point,
lineup, game, MMR 원장은 PostgreSQL에만 둔다.

## 확정 제품 규칙

- Discord OAuth2가 유일한 web 인증이다.
- guild_id가 모든 운영 데이터의 경계다.
- lane wire enum은 TOP/JUNGLE/MID/BOTTOM/SUPPORT다.
- 일반 내전과 경매 내전 모두 guild MMR에 반영한다.
- 경매는 TEN_PERSON 2팀과 TWENTY_PERSON 4팀을 지원한다.
- bestOf storage는 1, 3, 5를 지원한다. 일반 내전 모집은 생성 시 BO1/BO3/BO5와 하드 피어리스를
  선택하고, series는 그 설정을 immutable snapshot으로 복사한다. 경매 모집도 결승 BO1/BO3/BO5와 하드
  피어리스를 선택하며, 20인은 4강과 결승 BO를 독립적으로 저장해 tournament/match snapshot으로 복사한다.
- series/recruitment와 tournament/auction recruitment는 별도 ID와 unique FK다.
- custom match 결과에서 KDA/CS는 필수 입력이 아니다.
- 중요한 mutation은 expectedRevision과 PostgreSQL transaction을 사용한다.
- 범용 undo가 아니라 고정 checkpoint revert만 제공한다.
- 현재 게임/Riot Spectator는 MVP에서 제외한다.

## Phase 0. Frontend contract

현재 구현된 화면을 제품 표준으로 동결한다.

대상:

- 공통 shell, guild selector, 대표 길드 redirect
- operations queue
- 일반 내전 진행/완료, entry, pick-ban/result input, result ledger
- 경매 진행/완료, 10인/20인 setup, auction, bracket, lineup, result
- guild records, Riot records, leaderboard
- /me, minimal user profile
- guild admin, service status

완료 기준:

- 모든 control이 local-only, HTTP query, HTTP command 중 하나로 분류된다.
- route별 request/response golden fixture가 있다.
- OpenAPI와 WS event 초안이 현재 component 의미를 보존한다.
- 미사용 `lib/contracts/dashboard.ts`는 실사용 operations DTO와 혼합하지 않고 폐기 대상으로
  분류한다.
- operations의 `RESULT` 작업은 `/pick-ban`, 완료 원장은 `/result`로 연결한다.

## Phase 1. Axum/PostgreSQL foundation

- Rust workspace, config, error, tracing, health, graceful shutdown.
- SQLx migration, actual PostgreSQL integration harness.
- guild composite FK, revision, audit, checkpoint, idempotency, outbox.
- Discord OAuth session과 guild/operator guard.
- Bot service principal 인증 기반.

완료 기준:

- 빈 PostgreSQL migration과 restore smoke가 통과한다.
- cross-guild 접근과 참조가 차단된다.
- Discord login/logout과 대표 길드 조회가 동작한다.

## Phase 2. Read-only operating slice

연결 route:

- /g/{guildId}
- /g/{guildId}/series
- /g/{guildId}/auction
- /users/{discordId}

operations/actionable/active/completed projection을 먼저 연결한다. 화면을 API에
맞춰 다시 디자인하지 않고 현재 row/section 구조를 유지한다.

완료 기준:

- 위 route가 fixture 없이 PostgreSQL 데이터로 렌더링된다.
- 10인/20인 경매 행, empty state, 정렬, tenant 격리가 검증된다.

## Phase 2.5. 최소 Bot 운영 경로

Bot은 Node 22와 discord.js로 구성하고 PostgreSQL에 직접 접근하지 않는다.
Discord interaction은 Bot service principal로 Axum command API만 호출한다.

- `/초기설정`: 길드, 운영자 역할, 필수 모집 채널과 선택적 엔트리/결과/운영 로그 채널 설정.
- `/내전모집`: 일반 모집 생성과 Discord 메시지 연결.
- 모집 component: 참가, 취소, 선호 라인, 엔트리 시작.
- Discord interaction ID 기반 idempotency와 API readiness 확인.

완료 기준:

- seed 데이터 없이 실제 test guild interaction으로 모집 snapshot이 생성된다.
- 동일 interaction replay가 같은 응답을 반환하고 mutation은 한 번만 수행된다.
- Bot token이 없는 CI에서도 API/Bot contract test가 통과한다.

## Phase 3. 일반 내전

- recruitment snapshot과 entry operation.
- separate series 생성.
- pick/ban cell operation.
- game submit, v3 호환 MMR, score/completion.
- last game revert와 result ledger.
- Entry reset/reopen/createSeries와 PickBan submit/deleteLast/revert control 연결.
- 최소 Bot 모집 경로의 series created/completed notification.

현재 /pick-ban은 side, pick/ban, winner 입력 작업대이며 /result는 읽기
원장이다. pick/ban은 optional이고 game winner는 확정 시 필수다.

완료 기준:

- 신규 BO1/BO3/BO5 시리즈 완료와 imported BO1/BO3 조회.
- game/MMR/audit/checkpoint가 한 transaction.
- revert 후 MMR/score/status가 일치.
- Bot 모집부터 결과 알림까지 Axum command 경계를 재사용한다.

## Phase 4. 제한적 실시간 협업

WS route:

```text
/g/{guildId}/recruitments/{id}/entry
/g/{guildId}/series/{id}/pick-ban
/g/{guildId}/auction/recruitments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/bracket
```

HTTP가 mutation을 수행하고 WS는 outbox/Valkey를 통해 invalidate와 presence만
전달한다.

완료 기준:

- 같은 revision의 두 mutation 중 하나만 성공한다.
- 409 -> refetch -> 사용자 재적용 흐름이 동작한다.
- Valkey 장애에도 PostgreSQL 원장 저장은 정상이다.

## Phase 5. 경매 내전

- captain과 initial point를 한 setup command로 확정.
- draw, bid award, unsold, manual assignment, recent step revert.
- 2팀 결승과 4팀 준결승/결승 bracket.
- match lineup role assignment와 lock.
- game result, guild MMR, result revert.

완료 기준:

- 10인/20인 모두 setup부터 completed result까지 진행된다.
- 동시 낙찰은 정확히 하나만 commit된다.
- 완성/lock되지 않은 lineup으로 결과를 입력할 수 없다.

## Phase 6. Records와 self-service

Route:

- /g/{guildId}/records
- /records
- /g/{guildId}/leaderboard
- /users/{discordId}
- /me

guild records는 내부 game 원장이 기준이며 /records는 Riot 외부 전적이다.
/users/{discordId}는 현재 UI가 소비하는 minimal profile부터 구현한다.

완료 기준:

- 일반/10인/20인 기록과 expand detail이 재현된다.
- Riot 계정 add/delete/main/refresh, privacy, champion preference, 대표 길드가 동작한다.
- Riot 장애가 내부 전적/MMR을 방해하지 않는다.

## Phase 7. Guild admin

웹에 구현하는 것:

- member 조회와 ACTIVE/INACTIVE
- manual MMR adjustment ledger
- display/recruitment/entry/log channel/operator role/public 설정
- 현재/종료 시즌 집계 조회와 새 시즌 전환
- recent checkpoint revert
- audit logs
- API/PostgreSQL/Bot/Valkey status

웹에 만들지 않는 것:

- 일괄 등록
- 시즌 초기화
- 오래된 내전 정리
- 범용 maintenance dashboard

시즌은 settings payload와 분리된 `GET/POST /api/guilds/{guildId}/seasons`로 조회하고 전환한다. Web
운영자가 현재 시즌 ID를 concurrency token으로 보내며, 전환은 기존 시즌 종료, 새 시즌 생성, audit와
idempotency 응답 저장을 한 transaction으로 commit한다. 향후 Bot `/새시즌`도 별도 Bot 인증으로 같은
application command를 재사용한다.

완료 기준:

- 모든 admin mutation이 aggregate에 맞는 concurrency token을 사용한다.
- audit가 domain write와 같은 transaction에 남는다.

## Phase 8. Remaining Bot

Bot은 Discord command parsing, component interaction, message rendering,
notification에 집중한다. PostgreSQL에 직접 접근하지 않고 Axum API/application
service를 사용한다.

- Phase 2.5의 일반 모집 interaction/idempotency 기반을 재사용한다.
- 경매 모집과 운영 command.
- 사용자 조회와 web과 같은 admin/domain command.
- heartbeat와 season rotate mapping.

완료 기준:

- Web과 Bot의 같은 command가 같은 결과/audit를 만든다.
- duplicate interaction이 한 번만 실행된다.

## Phase 9. Migration과 hardening

- mookbot_v3 D1 및 routed Valkey draft read-only export, cutover write freeze.
- legacy guild/user/ID mapping.
- PostgreSQL staging의 aggregate-unit import/quarantine와 shadow read.
- count, orphan, tenant crossing, MMR/point 합계, active quarantine=0 검증.
- load/failure/security test.
- backup/restore/rollback rehearsal.
- backend dev 30020 배포.

과거 경매 MMR은 자동 소급하지 않는다. 별도 운영 결정을 먼저 기록한다.

완료 기준:

- mookbot_v3 source에 write가 없다.
- restore와 rollback 시간이 측정되어 있다.
- 기존 서비스 정상 상태를 유지한다.
- main 배포는 별도 승인 뒤 수행한다.

## 리스크

- frontend fixture를 그대로 DB row로 옮기면 display 문자열과 원장이 섞인다.
- guildId 없는 repository/API는 tenant 누출 위험이 있다.
- external HTTP를 transaction 안에서 호출하면 lock 시간이 길어진다.
- Valkey를 원장으로 쓰면 장애 복구와 정합성이 깨진다.
- 전체 JSON draft replace는 동시 편집 충돌 범위를 키운다.
- Bot이 DB에 직접 접근하면 Web과 다른 invariant/audit 경로가 생긴다.
- v3를 in-place migration하면 현 운영 서비스에 영향을 줄 수 있다.

## Non-goals

- v3 route/endpoint 전부 복사.
- 모든 문서 기능을 UI가 없는데 선제 구현.
- MMR 알고리즘 변경.
- 범용 realtime editing engine.
- 독립 디자인 시스템 package.
- 낮은 빈도의 debug/admin UI 복원.
