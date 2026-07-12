# MonkeyBot Implementation Roadmap

작성일: 2026-07-11

## 목적

이 문서는 저장소 전체의 구현 순서와 현재 기술 결정을 짧게 고정한다.
백엔드의 세부 task, transaction, test, migration, 배포 gate는
[Backend Development Plan](docs/09-backend-development-plan.md)을 따른다.

## 판단 기준

구현 판단 순서:

1. 현재 구현된 SvelteKit route/component/fixture/command 동작
2. 이 저장소의 docs
3. mookbot_v3의 검증된 domain 로직

프론트와 문서가 충돌하면 현재 프론트를 기준으로 문서를 고친다. v3는 복사
대상이 아니라 MMR, migration, Discord 운영 동작의 reference다.

## 확정 기술

```text
Frontend       SvelteKit frontend only
Backend        Rust + Axum
Database       PostgreSQL
DB access      SQLx
Realtime       Axum WebSocket + Valkey Pub/Sub
Auth           Discord OAuth2 only
Bot            Node 22 + discord.js, Axum API client only
Deploy         VPS + Docker Compose + nginx
Design         Tailwind CSS + Bits UI + lucide-svelte
```

폐기:

- SvelteKit fullstack backend
- Cloudflare D1/KV
- Drizzle backend
- Fastify/Node API
- Discord Activity
- 별도 회원가입

## 개발 순서

```text
1. Frontend UI와 workflow contract 확정
2. Axum/PostgreSQL foundation, 인증, 조회 경계
3. 최소 Bot으로 길드 설정과 일반 모집 운영 데이터 생성
4. 현재 UI의 fixture를 API adapter로 교체
5. 일반/경매 vertical slice와 제한적 WebSocket 연결
6. 나머지 Bot 명령과 알림 연결
7. legacy migration rehearsal
8. hardening과 dev 배포
9. 별도 승인 후 main 배포
```

현재 프론트 UI는 백엔드 계약의 기준으로 사용할 수 있는 단계다. Axum 원장과
command 경계를 먼저 만들되, fixture를 운영 데이터처럼 늘리지 않는다. 실제
Discord 길드에서 데이터를 만들 수 있도록 최소 Bot 운영 경로를 인증 직후에
연결하고 나머지 Bot 기능은 해당 vertical slice와 함께 확장한다.

## 현재 Frontend 기준

핵심 route:

```text
/                                      대표 길드 redirect
/g/{guildId}                           operations queue
/g/{guildId}/series                    일반 내전 진행/완료
/g/{guildId}/recruitments/{id}/entry   엔트리 편성
/g/{guildId}/series/{id}/pick-ban      픽밴/결과 입력
/g/{guildId}/series/{id}/result        일반 결과 원장
/g/{guildId}/auction                   경매 진행/완료
/g/{guildId}/auction/**                10인/20인 경매 작업대
/g/{guildId}/records                   길드 내부 전적
/records                               Riot 외부 전적
/g/{guildId}/leaderboard               길드 랭킹
/users/{discordId}                     사용자 프로필
/me                                   내 정보/계정/개인정보/대표 길드
/g/{guildId}/admin                     길드 관리
/admin/status                          서비스 운영 상태
```

고정 domain rule:

- 모든 운영 데이터는 guild_id로 격리한다.
- lane은 TOP/JUNGLE/MID/BOTTOM/SUPPORT다.
- 일반/경매 게임 모두 guild MMR에 반영한다.
- 경매는 TEN_PERSON 2팀과 TWENTY_PERSON 4팀을 지원한다.
- series와 recruitment, tournament와 auction recruitment는 별도 ID/FK다.
- custom match의 KDA/CS는 필수 입력이 아니다.
- 결과/낙찰/MMR/revert는 PostgreSQL transaction으로 확정한다.
- Valkey는 원장이 아니다.

## Phase 0. Contract freeze

- route/component control을 query/command/local-only로 분류한다.
- frontend fixture 의미를 golden API fixture로 옮긴다.
- guild-scoped API path, error, revision, time, ID 규칙을 고정한다.
- OpenAPI/WS schema 초안을 만든다.
- local-only component callback backlog를 만든다.

완료 기준:

- API 연결에 UI 재설계가 필요 없다.
- 현재 모든 버튼과 expand detail의 server owner가 명확하다.

## Phase 1. Axum/PostgreSQL foundation

- Cargo workspace와 apps/api를 만든다.
- config, error, tracing, health, graceful shutdown을 구현한다.
- SQLx migration과 실제 PostgreSQL test harness를 만든다.
- tenant composite FK, idempotency, outbox 기반을 만든다.

완료 기준:

- fmt/clippy/test와 빈 DB migration이 통과한다.
- cross-guild 참조가 DB와 repository 양쪽에서 차단된다.

## Phase 2. Auth와 read vertical slice

- Discord OAuth/session/CSRF를 구현한다.
- guild membership/operator/service-admin guard를 구현한다.
- /me, guild context, operations, series/auction list를 연결한다.
- 대표 길드를 PostgreSQL에 저장한다.

완료 기준:

- 실제 Discord test login/logout이 동작한다.
- dashboard/series/auction 목록이 fixture 없이 렌더링된다.

## Phase 2.5. 최소 Bot 운영 경로

- Bot service principal과 Discord interaction idempotency를 구현한다.
- `/초기설정`으로 길드, 운영자 역할, 모집 채널을 등록한다.
- `/내전모집`과 참가/취소/선호 라인/엔트리 시작 interaction을 연결한다.
- Bot은 PostgreSQL에 직접 접근하지 않고 Axum API만 호출한다.
- 개발 DB의 운영 데이터는 seed 대신 실제 Discord interaction으로 생성한다.

완료 기준:

- 실제 test guild에서 설정과 일반 모집 snapshot이 생성된다.
- 같은 Discord interaction 재전송이 mutation을 중복 실행하지 않는다.
- Bot token 없이도 API contract/integration test는 독립 실행된다.

## Phase 3. 일반 내전 vertical slice

- entry operation, series 생성, pickban operation을 연결한다.
- game 결과, v3 호환 MMR, last-game revert를 구현한다.
- 일반 결과 ledger와 records projection을 만든다.
- 최소 Bot 모집 경로에 series 생성/완료 알림을 연결한다.

완료 기준:

- BO1/BO3/BO5 완료, MMR 반영, revert가 한 transaction 기준으로 검증된다.

## Phase 4. Limited realtime

- entry와 pick-ban에 revision/409를 연결한다.
- transactional outbox, Valkey Pub/Sub, /ws invalidate를 구현한다.
- reconnect/refetch/conflict와 presence를 연결한다.

완료 기준:

- 두 운영자의 같은 revision mutation 중 하나만 성공한다.
- Valkey 장애에도 HTTP 원장은 정상이다.

## Phase 5. 경매 vertical slice

- 10인/20인 captain/point, draw, bid, unsold, manual assign을 구현한다.
- 2팀/4팀 bracket, lineup lock, result/MMR/revert를 구현한다.
- auction recruitment/tournament/match WS를 연결한다.

완료 기준:

- 두 경매 형식이 end-to-end 완료된다.
- 동시 낙찰 하나만 commit되고 불완전 lineup 결과가 차단된다.

## Phase 6. 조회와 self-service

- guild records, leaderboard, profile을 연결한다.
- Riot accounts, champion preferences, privacy, default guild를 연결한다.
- /records 외부 Riot 조회와 cache/rate limit을 구현한다.

완료 기준:

- 현재 일반/10인/20인 expand 구조를 실제 데이터로 재현한다.
- Riot 장애가 내부 MMR/전적을 방해하지 않는다.

## Phase 7. Admin과 나머지 Bot

- 현재 admin의 member/MMR/settings/revert/audit만 연결한다.
- 최소 Bot에서 만든 service auth와 idempotency 경계를 재사용한다.
- heartbeat, notification outbox, 경매/조회/admin command를 연결한다.

완료 기준:

- Web/Bot의 같은 작업 결과와 audit가 일치한다.
- Bot이 PostgreSQL에 직접 접근하지 않는다.

## Phase 8. Migration/hardening

- mookbot_v3를 read-only export하고 PostgreSQL staging import를 검증한다.
- count/MMR/orphan/tenant crossing을 검증한다.
- load/failure/security/backup/restore/rollback rehearsal을 수행한다.
- backend dev 30020만 먼저 배포한다.

완료 기준:

- 기존 mookbot_v3를 수정하지 않는다.
- PostgreSQL restore와 Valkey 장애 복구가 검증된다.
- main 배포는 별도 승인 전 실행하지 않는다.

## WS 범위

MVP WS route:

```text
/g/{guildId}/recruitments/{id}/entry
/g/{guildId}/series/{id}/pick-ban
/g/{guildId}/auction/recruitments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/bracket
```

dashboard, 목록, records, leaderboard, profile, admin, 읽기 전용 result에는
MVP WS를 붙이지 않는다.

## 배포 port

```text
10010 PostgreSQL
20010 frontend main
20020 backend main
30010 frontend dev
30020 backend dev
```

PostgreSQL은 loopback/private Docker network만 사용한다. main/dev는 별도
database와 role을 사용한다. 기존 mookbot_v3 nginx/site/upstream은 유지한다.

## 기본 검증

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm check
pnpm test
pnpm build
```

`cargo sqlx prepare --check`는 `query!` 계열 compile-time macro를 도입한 시점부터 migration이 적용된
임시 PostgreSQL을 대상으로 실행하고 `.sqlx` metadata를 commit한다. 동적 SQLx API만 사용하는 현재
단계는 실제 PostgreSQL integration test가 SQL 검증 gate다.

Frontend dev server는 다른 shell이 담당한다. Codex는 명시적 지시 없이는
dev server를 시작하지 않는다.
