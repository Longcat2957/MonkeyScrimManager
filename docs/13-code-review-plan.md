# Code Review Plan

작성일: 2026-07-15

## 목적과 기준선

이 계획은 MonkeyBot의 현재 구현을 검토하기 위한 절차다. 과거 roadmap이나 미등록 Bot command를 기대 동작으로
삼지 않는다.

리뷰 시작 시 기준선은 해당 시점의 commit과 다음 runtime surface로 고정한다.

1. `apps/web/src/routes`, `apps/web/src/lib/api`, `apps/web/src/lib/contracts`
2. `apps/api/src/http/mod.rs`, handler/service/repository, migration
3. `apps/bot/src/commands/index.ts`, command handler, interaction dispatcher
4. [Current Operating Contract](12-current-operating-contract.md) 및 현재 route 계약 문서

문서와 구현이 다르면 우선 문서 drift로 기록한다. 구현이 현재 source 기준의 workflow, 권한, 데이터 일관성을
깨뜨릴 때만 코드 결함으로 보고한다.

리뷰 중에는 코드를 수정하지 않는다. 발견 사항을 보고한 뒤 승인된 항목만 별도 수정 작업으로 진행한다.

## 검토 순서

### 1. 공개 계약과 경계

- SvelteKit route, same-origin proxy, Axum route registration, frontend API adapter, Bot API client가 같은
  URL·method·request/response shape를 사용하는지 확인한다.
- 인증 실패, 권한 실패, validation, `404`, `409`, upstream 장애가 요청 ID와 사람이 읽을 수 있는 이유를
  보존하는지 확인한다.
- `expectedRevision`, CSRF, `Idempotency-Key`가 필요한 mutation에 빠지지 않았는지 확인한다.
- UI가 API에 없는 path나 더미 DTO를 운영 route에서 호출하지 않는지 확인한다.

### 2. 일반 내전 lifecycle

- 모집 생성, 참가/취소, roster 수동 조작, 마감, 엔트리, series 생성, 픽밴, 세트 확정, 결과, recovery를
  상태 전이 순서로 추적한다.
- BO1/BO3/BO5, hard fearless, 실제 BLUE/RED 선택, 일괄 입력, 조기 종료에서 rule snapshot이 유지되는지 본다.
- 마지막 확정 game의 revert가 score, MMR ledger, audit, Discord feedback을 불완전하게 남기지 않는지 확인한다.
- capability가 없는 사용자가 버튼/endpoint로 lifecycle을 우회하지 못하는지 확인한다.

### 3. 경매 내전 lifecycle

- 10인과 20인 모집 규칙, captain/시작 point, BIDDING, 유찰/낙찰/수동 배치/revert, bracket, lineup, match,
  tournament 결과를 처음부터 끝까지 검토한다.
- 20인에서 준결승·결승 best-of snapshot과 20명 lineup lock gate가 각 match에 정확히 적용되는지 확인한다.
- 입찰 action과 point, team membership, tournament revision이 한 transaction에서 일관되게 바뀌는지 본다.
- 브래킷 확정 해제, 입찰 단계 복귀, lineup unlock, last-game revert, 취소/조기 종료의 안전 조건과 MMR tail
  검증을 확인한다.

### 4. tenancy, identity, privacy, dummy user

- 모든 guild-scoped read/write가 `guildId` 경계를 지키고 다른 guild resource를 노출하지 않는지 확인한다.
- Discord snowflake가 canonical user identity이며 nickname/global display name이 key로 쓰이지 않는지 본다.
- Riot PUUID 연결, Riot ID 공개 설정, guild records 검색, OAuth session과 Bot actor가 privacy scope를
  우회하지 않는지 확인한다.
- dummy user가 database identity로 동작하되 service-admin 외 운영자와 일반 사용자에게 기능이나 유형이
  노출되지 않는지 확인한다.

### 5. 제한적 realtime

- WebSocket 연결 대상이 일반 entry, 일반 pick-ban, 경매 모집 roster, `BIDDING` 경매 draft로 한정되는지
  확인한다.
- operator-only topic, 일반 멤버의 read-only behavior, reconnect/unsubscribe, stale/out-of-order invalidate,
  dirty local draft와 `409` refetch를 점검한다.
- 경매 setup, bracket, lineup, match, result route가 socket을 열지 않고 HTTP snapshot과 revision conflict로
  동작하는지 확인한다.
- WebSocket이 state patch, editor presence, mutation queue 또는 lock의 source of truth로 사용되지 않는지
  확인한다.

### 6. 데이터·외부 연동·운영 안정성

- migration이 repository query와 DTO projection을 지원하며 cascade/restrict 정책이 recovery에 맞는지 확인한다.
- transaction, audit, idempotency record, transactional outbox, MMR ledger가 원자적으로 commit되는지 본다.
- Discord notification relay의 retry/ack와 API 또는 Bot 장애 시 domain write 경계를 확인한다.
- Riot 조회 cache/refresh cooldown이 PostgreSQL snapshot을 우선하며 privacy 또는 guild records와 섞이지
  않는지 점검한다.
- Valkey 장애가 PostgreSQL 원장 write를 막지 않고 invalidate 전달만 degraded 처리하는지 확인한다.

### 7. Frontend 상태와 UI 회귀

- route load와 deep link에서 loading/error/empty/auth transition이 실제 API contract와 맞는지 확인한다.
- Svelte `$effect`, `$derived`, parent controlled prop, local draft, modal/expand state가 route 전환에서
  무한 갱신이나 stale state를 만들지 않는지 본다.
- 공유 픽밴/브래킷/entry 컴포넌트가 일반 내전과 경매 내전에서 같은 action을 임의로 숨기거나 바꾸지 않는지
  확인한다.
- 이 track은 기능·권한·반응형 레이아웃 회귀만 다룬다. 요청되지 않은 visual redesign이나 UI 기능 추가는
  리뷰 권고로도 제안하지 않는다.

## 검증 게이트

리뷰는 처음부터 전체 test suite를 반복 실행하지 않는다. 먼저 정적 계약과 재현 가능한 고위험 경로를 확인하고,
발견된 위험에 비례해 명령을 선택한다.

| 상황 | 최소 검증 |
| --- | --- |
| 모든 리뷰 시작/종료 | `git diff --check` |
| Web route·component·Markdown import 관련 | `pnpm --filter @monkeybot/web check` |
| API/Rust 경로 관련 | `cargo fmt --all --check`, 필요한 package의 targeted test 또는 `cargo test -p monkeybot-api` |
| Bot command/interaction 관련 | 해당 `apps/bot` Vitest 대상 |
| migration/outbox/recovery 관련 | 해당 integration test 또는 migration smoke를 별도 명시 |

실행하지 않은 검증은 결과 보고에 명시한다. frontend dev server는 이 계획의 검증 수단이 아니며, 이미 실행 중인
사용자 환경에서만 필요 시 시각 확인을 요청한다.

## 결과 보고 형식

발견 사항은 심각도 순서로 아래 정보만 포함한다.

1. 심각도와 한 줄 제목
2. 파일과 정확한 line 또는 route
3. 재현 조건과 실제 영향
4. 현재 runtime contract 기준으로 왜 잘못됐는지
5. 권장 수정 방향과 필요한 targeted test

문서 drift, 테스트 공백, 확인하지 못한 외부 상태는 코드 결함과 분리한다. 문제가 없으면 `발견 사항 없음`을
명확히 쓰고, 남은 검증 공백과 잔여 위험만 짧게 덧붙인다.
