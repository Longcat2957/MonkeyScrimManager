# MonkeyScrimManager Roadmap

작성일: 2026-07-10

## 방향

이 로드맵은 `../mookbot_v3`를 새 repository로 단순 복사하는 계획이 아니다. v3의 동작을 기준선으로 삼아 도메인 계약을 먼저 고정하고, core/API/bot/web을 새 구조로 재작성하는 계획이다.

우선순위는 다음 순서다.

1. 기능 범위와 도메인 계약을 확정한다.
2. DB/Core/API의 신뢰성 있는 상태 전이를 먼저 세운다.
3. Discord bot은 기존 사용자-facing 명령을 유지하되 슬림하게 다시 작성한다.
4. Web console은 운영 작업대 중심으로 새로 설계한다.
5. v3 데이터와 운영 배포 경로를 안전하게 연결한다.

## 목표 구조

```txt
MonkeyScrimManager/
  apps/
    bot/          # discord.js command, component handler, notification
    api/          # HTTP API, auth, permission, websocket, bot notify
    web/          # SvelteKit 운영 콘솔
  packages/
    core/         # DB repository, domain service, Riot, Data Dragon, KV, MMR
    contracts/    # zod schema, shared DTO, route contracts
    api-client/   # typed fetch client for web and tests
  docs/
  scripts/
  infra/
```

초기에는 `contracts`와 `api-client`를 작게 시작한다. 모든 타입을 한 번에 추출하지 않고, dashboard, recruitment, series, auction 순서로 실제 route가 필요로 하는 계약부터 고정한다.

## Phase 0: 프로젝트 기준선

목표: 빈 프로젝트를 실행 가능한 TypeScript workspace로 만든다.

작업:

- root `package.json`, `pnpm-workspace.yaml`, TypeScript/Vitest 기본 설정 추가.
- `apps/api`, `apps/bot`, `apps/web`, `packages/core`, `packages/contracts`, `packages/api-client` scaffold 생성.
- lint/format/typecheck/test script의 최소 기준 확정.
- `.env.example` 작성: Discord, Riot, DB, session, web URL, internal token.
- v3 참조 문서와 새 프로젝트 문서의 관계를 `docs/`에 명시.

완료 기준:

- `pnpm install` 후 root에서 typecheck/test 명령이 존재한다.
- 빈 앱이라도 api health, bot env validation, web shell이 실행 가능하다.
- Activity SDK, React Activity, shadcn-svelte, daisyUI, Skeleton UI dependency가 없다.

## Phase 1: 도메인 계약 추출

목표: 구현 복사 전에 반드시 지켜야 할 상태와 API 계약을 분리한다.

작업:

- v3 DB schema에서 핵심 entity를 추출한다.
- recruitment, series, game, user, riot account, auction recruitment, auction tournament, audit log의 상태 모델을 문서화한다.
- `packages/contracts`에 공통 enum, ID type, DTO schema를 작성한다.
- WebSocket topic naming을 고정한다.
- destructive/admin action의 audit payload 기준을 정한다.
- 고정 단계 revert checkpoint 기준을 정한다.

참조:

- `../mookbot_v3/packages/core/src/db/schema.sql`
- `../mookbot_v3/apps/api/src/http`
- `../mookbot_v3/docs/v2-feature-inventory.md`

완료 기준:

- contracts 테스트가 core/API 없이 단독 실행된다.
- 일반 내전과 경매 내전의 주요 상태 전이가 타입으로 표현된다.
- v3와 달라지는 필드가 migration note에 기록된다.

## Phase 2: Core 이식과 정리

목표: DB, MMR, Riot, Data Dragon, KV 로직을 새 프로젝트 기준으로 가져온다.

작업:

- v3 `packages/core/src/db`에서 필요한 repository를 선별 이식한다.
- MMR/ELO, game stats, picks/bans 저장 로직을 테스트와 함께 가져온다.
- Riot API client, Data Dragon client, screening logic은 모듈 경계를 재정리해서 이식한다.
- KV는 `guild_kv`와 Redis/Valkey routed KV를 유지하되 interface를 단순화한다.
- seed, backup, restore, migration script를 새 scripts 구조에 맞춘다.

유지할 규칙:

- 일반 내전은 MMR에 반영한다.
- 경매 내전도 MMR에 반영한다.
- `series.id === recruitments.id`를 유지한다.
- `auction_tournaments.id === auction_recruitments.id`를 유지한다.
- 주요 삭제는 soft-delete 또는 상태 전이로 보존한다.

완료 기준:

- core unit test가 v3 기준 동작을 검증한다.
- DB migration을 빈 DB와 v3 호환 DB 양쪽에서 검증할 수 있다.
- domain helper가 API나 bot framework에 의존하지 않는다.

## Phase 3: API 재작성

목표: Fastify API를 새 contracts 위에 다시 세운다.

작업:

- session/auth: Discord OAuth2 only, signed httpOnly `sid`, logout, `/api/me`, `/api/me/perms`.
- recruitment API: 목록, 상세, entry draft, reopen, series 생성.
- series API: 상세, pickban draft, game result, revert, completed list, balance SVG.
- auction API: recruitment, tournament, bidding, bracket, match game result.
- user API: search, profile, MMR history, preferences, Riot accounts.
- admin API: logs, health, permission diagnostics, destructive action endpoints.
- WebSocket invalidate server와 topic subscription 구현.
- API -> bot internal notify endpoint를 유지한다.
- 고정 단계 revert API를 구현한다.

설계 원칙:

- route handler는 얇게 두고 상태 전이는 core service/repository로 보낸다.
- audit log 기록을 API boundary에서 누락하지 않는다.
- 응답 shape는 `packages/contracts` 기준으로 검증한다.

완료 기준:

- v3의 주요 API surface와 parity matrix가 작성된다.
- 핵심 mutation은 test harness DB로 검증된다.
- API health, auth, recruitment, series, auction, logs 테스트가 통과한다.
- 이메일/비밀번호 회원가입, 자체 로그인, 추가 OAuth provider가 없다.

## Phase 4: Bot 슬림 재작성

목표: 기존 Discord UX는 유지하되, 봇 내부 로직을 작게 만든다.

작업:

- slash command 이름과 option을 v3 기준으로 등록한다.
- 사용자 명령: `/도움말`, `/등록`, `/내정보`, `/내정보갱신`, `/내전기록`, `/랭킹`, `/전적`, `/지금게임`, `/전적검토`.
- 일반 내전 운영 명령: 모집, 인원 추가/삭제, 랜덤 추가, 목록, 복구, 강제 삭제, 조기 종료.
- 경매 내전 운영 명령: 모집, 인원 추가/삭제, 목록, 강제 삭제, 조기 종료.
- 관리자 명령: 일괄등록, MMR 수정, 시즌 리셋, 오래된 내전 정리, 로그, 유저 강제 삭제.
- 운영자 명령: 최근 작업 되돌리기.
- recruitment/auction message builder와 button/select handler를 새 API client 기준으로 작성한다.
- 웹 링크는 `WEB_BASE_URL` route로 발급한다.
- 라인업 이미지와 종료 카드 발행은 best-effort로 유지한다.

정리 대상:

- 봇에서 직접 DB 상태를 많이 바꾸는 코드.
- Activity 안내 문구.
- command별 중복 permission/audit helper.

완료 기준:

- command deploy가 새 프로젝트에서 실행된다.
- button/select interaction test가 주요 메시지 상태를 검증한다.
- 운영자 명령은 API/Core 계약을 통해 상태를 바꾼다.

## Phase 5: Web 기반 설계

목표: 운영 콘솔의 화면 원칙과 공통 primitive를 먼저 세운다.

작업:

- SvelteKit, Tailwind CSS, Bits UI, lucide 기반 scaffold 구성.
- `MookPageShell`, `MookPageHeader`, `MookPanel`, `MookDataRow`, `MookStatusBadge`, `MookActionMenu` 같은 최소 primitive 작성.
- nested card, hero, generic SaaS metric dashboard 금지 규칙을 컴포넌트 문서에 반영한다.
- API client, auth store, websocket invalidate store를 작성한다.
- loading, empty, error, unauthorized 상태를 공통화한다.

완료 기준:

- 첫 화면 shell이 실제 운영 queue 구조를 담을 수 있다.
- route-local 색/spacing 보정 없이 기본 primitive로 화면을 만들 수 있다.
- `apps/web`에는 React, Activity SDK, shadcn-svelte, daisyUI, Skeleton UI가 없다.

## Phase 6: 일반 내전 Web Route

목표: 일반 내전 운영을 웹에서 끝까지 처리한다.

Route:

- `/`
- `/recruitments`
- `/recruitments/[id]/entry`
- `/series`
- `/series/[id]/pick-ban`
- `/series/[id]/result`
- `/series/completed`

작업:

- dashboard는 처리 대기, 진행 중, 최근 완료 queue로 구성한다.
- recruitment list/detail은 row/table 중심으로 만든다.
- entry editor는 full-width 작업대로 구성한다.
- pick-ban은 game tabs, side selector, team board, champion command list, result submit rail로 구성한다.
- result 화면은 게임 원장, picks/bans, MMR 변화, 종료 상태를 명확하게 보여준다.

완료 기준:

- 일반 모집 생성 후 웹에서 시리즈 완료까지 진행 가능하다.
- draft autosave와 WebSocket invalidate가 동작한다.
- 주요 desktop/mobile viewport에서 레이아웃이 깨지지 않는다.

## Phase 7: 사용자, 랭킹, Riot 계정

목표: 운영자가 사용자 상태를 확인하고 유저가 Riot 계정을 관리할 수 있게 한다.

Route:

- `/g/[guildId]/records`
- `/users`
- `/users/[discordId]`
- `/leaderboard`
- `/me/riot-accounts`

작업:

- records search는 Discord user, Riot ID, guild 표시 이름 검색을 지원한다.
- records result는 내부 내전 전적을 중심으로 두고 Riot 외부 요약을 보조로 붙인다.
- user search는 command/list 중심으로 구현한다.
- profile은 MMR, 최근 게임, 챔피언 선호, 경매 카드 정보를 dense section으로 구성한다.
- leaderboard는 라인별/통합 view를 제공한다.
- Riot 계정 추가/삭제/메인 지정/갱신을 구현한다.

완료 기준:

- guild 내부 전적 검색은 `games.guild_id` 기준으로 격리된다.
- Discord 등록 사용자와 Riot account가 연결되어 표시된다.
- profile과 leaderboard가 v3 DB 데이터를 기준으로 읽힌다.
- 계정 관리 mutation은 audit 또는 user action log 기준을 충족한다.

## Phase 8: 경매 내전 Web Route

목표: 경매 모집부터 결과까지 웹에서 처리한다.

Route:

- `/auction`
- `/auction/recruitments/[id]/draft`
- `/auction/tournaments/[id]/draft`
- `/auction/tournaments/[id]/bracket`
- `/auction/tournaments/[id]/result`

작업:

- team captain selection, point allocation, bidding queue, bid intent, finalize/manual assign/revert를 구현한다.
- bracket setup, match별 라인업 확정, auction match game result 입력을 구현한다.
- 경매 match는 게임 시작 전에 양 팀 TOP/JUNGLE/MID/ADC/SUPPORT 라인업을 반드시 확정한다.
- 경매는 DB 원장을 source of truth로 삼고 UI local state를 원장처럼 쓰지 않는다.
- 팀/매치 entity tile은 허용하되 card pile layout은 피한다.

컴포넌트 작성 현황:

- 완료: `AuctionTournamentCaptainPointSetupBoard`
  - 경매 시작 전 팀장 선택과 팀별 시작 포인트 설정.
- 완료: `AuctionTournamentBidDraftWorkbench`
  - 매물 추첨, 입찰, 유찰, DND 기반 팀 배치.
- 완료: `AuctionTournamentBracketMatchSetupBoard`
  - 20인/4팀 경매의 대진 설정.
- 완료: `AuctionMatchStartingLineupInputBoard`
  - match별 5인 라인업 입력과 확정.
- 완료: `AuctionTournamentOverviewBoard`
  - 대진 결과, match별 스코어, 팀 구성, 참가자별 포인트/MMR 정산을 보여주는 경매 진행 요약 화면.
- 다음 작업: `AuctionMatchSummaryCard`
  - 브래킷 화면에서 match를 compact하게 보여주는 카드. 팀 vs 팀, 라인업 확정 여부, 결과 입력 진입만 포함한다.
- 통합 처리: `AuctionPlayerLedgerBoard`
  - 별도 컴포넌트로 만들지 않고 `AuctionTournamentOverviewBoard`의 참가자 정산 섹션에 포함한다.

컴포넌트 제외 기준:

- 별도 `AuctionMatchResultBoard`는 만들지 않는다. 게임 단위 결과 입력은 기존 결과 입력 컴포넌트 흐름을 사용한다.
- 별도 readiness/control dashboard는 만들지 않는다. 경매 진행 요약은 읽기 전용 원장으로만 두고, 조작은 각 단계 화면에서 처리한다.
- match 진행 전 확인 UI가 필요하면 독립 dashboard가 아니라 `AuctionMatchSummaryCard` 안의 최소 상태 표시로 제한한다.

완료 기준:

- 10인/20인 경매 흐름을 완료할 수 있다.
- 라인업 미확정 match는 결과 입력으로 넘어갈 수 없다.
- 입찰, 라인업 확정, 경매 결과 입력은 이전 단계로 되돌릴 수 있다.
- 경매 게임 기록도 일반 내전과 같은 guild MMR 원장에 반영된다.
- tournament result와 bot 종료 알림이 연결된다.

## Phase 9: Admin, 운영 안전장치, 배포

목표: 실제 운영에 필요한 진단, 감사, 배포 경로를 완성한다.

작업:

- `/admin/logs`, `/admin/status`, `/admin/perms` 구현.
- `/admin/logs`에서 최근 revert checkpoint 조회와 되돌리기 실행을 지원한다.
- healthz/deep healthz, internal heartbeat, bot notify status 점검.
- Dockerfile: api, bot, web.
- deployment script: build, push, VPS deploy, command registration.
- migration preflight와 rollback/backup 절차 문서화.
- v3 production 데이터 이전 또는 같은 DB 사용 전략 확정.

완료 기준:

- staging 또는 VPS에서 bot/api/web/nginx/valkey 구성이 올라온다.
- 배포 전 DB migration, 배포 후 health check, slash command deploy가 문서화된다.
- 운영자가 audit log로 주요 destructive action을 추적할 수 있다.
- 운영자가 최근 실수 작업을 웹 또는 Bot에서 되돌릴 수 있다.

## Phase 10: Parity Audit And Cutover

목표: v3 대비 누락과 불필요한 기능을 최종 판정하고 전환한다.

작업:

- v3 feature inventory 기준 parity matrix 작성.
- 명령어별 동작 확인.
- API endpoint별 사용 여부 확인.
- Web route별 desktop/mobile smoke test.
- 운영 시나리오 rehearsal:
  - 일반 내전 모집부터 완료.
  - 경매 내전 모집부터 완료.
  - 유저 등록/Riot 계정 갱신.
  - 관리자 강제 삭제/복구.
  - audit log 확인.
- cutover checklist 작성.

완료 기준:

- 필수 기능 누락이 없다.
- 유지하지 않기로 한 기능은 문서에 명시되어 있다.
- v3에서 새 프로젝트로 운영 전환할 수 있다.

## 우선 구현 순서

처음 2주 기준의 현실적인 순서는 다음과 같다.

1. root workspace와 package scaffold 생성.
2. `docs/`에 contracts/parity matrix 초안 추가.
3. core DB schema와 repository 최소 이식.
4. auth/session 없는 API health와 read-only dashboard API 작성.
5. web shell과 dashboard queue 화면 작성.
6. bot env/command deploy scaffold 작성.
7. recruitment create/list/detail 흐름 연결.
8. entry editor route 구현.
9. series create/pickban/result route 구현.
10. 일반 내전 E2E smoke 후 경매로 확장.

## 리스크

- v3 코드를 너무 많이 복사하면 새 프로젝트의 목적이 사라진다.
- contracts 없이 화면부터 만들면 v3와 같은 route-local 임시 코드가 다시 쌓인다.
- 봇이 DB를 직접 많이 만지면 API/Core 경계가 흐려진다.
- 경매 상태를 UI state 중심으로 만들면 복구와 동시성 문제가 커진다.
- 디자인을 먼저 과하게 만들면 운영 queue/workspace가 묻힌다.

## 당장 하지 않을 일

- v3 `apps/web` route를 그대로 복사.
- Activity 관련 dependency나 문구 복원.
- 모든 v3 endpoint를 한 번에 이식.
- 독립 디자인 시스템 패키지화.
- 사용 빈도가 낮은 debug/admin 화면까지 1차 버전에 포함.
- MMR 알고리즘 변경.
