# Frontend Routing

작성일: 2026-07-10

## 범위

이 문서는 `apps/web`의 SvelteKit route만 다룬다.

SvelteKit은 `src/routes` 디렉토리 기반의 파일 시스템 라우팅을 사용한다. 따라서 이 문서의 URL은 구현 시 `apps/web/src/routes/**/+page.svelte` 또는 `+server.ts` 구조로 대응한다.

Frontend route의 책임:

- 사용자에게 보이는 page URL.
- route별 초기 데이터 요청.
- same-origin proxy route.
- WebSocket invalidate 수신 후 화면 데이터 재조회.
- Bot이 제공한 딥링크의 landing page.

Frontend route의 비책임:

- 도메인 상태 전이의 최종 판단.
- operator/admin 권한의 최종 판단.
- audit log 기록.
- Discord interaction 처리.

인증 UI 정책:

- 로그인 시작은 Discord OAuth2로만 제공한다.
- 별도 회원가입 화면은 만들지 않는다.
- 이메일/비밀번호 로그인 화면은 만들지 않는다.
- 계정 생성 form, password reset form, email verification flow는 만들지 않는다.
- Riot 계정 연결 화면은 회원가입이 아니라 Discord 로그인 이후의 프로필 연결 기능이다.

## App Shell

### `/`

파일:

- `apps/web/src/routes/+page.svelte`

목적:

- 전역 app shell 확인용 루트 화면.
- 상단 navigation, guild selector, command/search, status strip, 본문 폭을 기능 화면 없이 확인한다.
- 실제 workflow 컴포넌트 샘플은 `/dev/components`에서 확인한다.

Backend 데이터:

- 없음.

WebSocket:

- 없음.

### `/dev/components`

파일:

- `apps/web/src/routes/dev/components/+page.svelte`

목적:

- 공통 UI 컴포넌트와 운영 화면 샘플을 확인하는 개발용 화면.
- root route를 실제 기능 샘플로 오염시키지 않고, panel/list/status/champion face 밀도를 검증한다.
- 운영자가 보는 최종 용어 기준에 맞춰 한국어 표시를 우선한다.

Backend 데이터:

- Data Dragon champion catalog fixture 또는 `/dd` proxy.

주의:

- 실제 운영 dashboard가 아니다.
- product workflow 구현이 진행되면 이 화면은 component regression 확인 용도로 유지한다.

### `/dev/fab`

파일:

- `apps/web/src/routes/dev/fab/+page.svelte`

목적:

- 전역 FAB와 관련 modal을 격리해서 확인하는 개발용 화면.
- 루트 화면이나 실제 workflow 화면과 겹치지 않게, 미니게임과 빠른 전적검색 진입 UX를 검증한다.

현재 작성된 범위:

- FAB 열기/닫기.
- 미니게임 modal.
- 전적검색 modal.
- 코인토스.
- 룰렛.
- 사다리.
- mock 기반 빠른 전적검색 UI.

주의:

- 실제 운영 route가 아니다.
- 미니게임 결과와 전적검색 결과는 mock data 기준이다.
- 실제 Riot 전적과 guild 내전 전적 동시 조회는 아직 API에 연결되어 있지 않다.

### `+layout.svelte`

파일:

- `apps/web/src/routes/+layout.svelte`

목적:

- 공통 shell, navigation, auth bootstrap, toast/overlay root.
- 로그인 상태와 현재 권한 요약을 표시한다.

주의:

- route-specific 데이터를 root layout에 과하게 올리지 않는다.
- protected data는 각 page load 또는 API client에서 다시 검증한다.

### `+error.svelte`

파일:

- `apps/web/src/routes/+error.svelte`

목적:

- 404, 401, 403, 500 등 사용자-facing 오류 화면.

## Proxy Routes

### `/oauth/[...path]`

파일:

- `apps/web/src/routes/oauth/[...path]/+server.ts`

목적:

- 같은 origin에서 Discord OAuth 시작/콜백을 Backend API로 proxy한다.
- 예: `/oauth/discord/start`, `/oauth/callback`.

주의:

- 실제 OAuth 처리와 session cookie 발급은 `apps/api`가 담당한다.

### `/api/[...path]`

파일:

- `apps/web/src/routes/api/[...path]/+server.ts`

목적:

- web app에서 API를 same-origin `/api/*`로 호출하기 위한 proxy.
- local/dev 환경과 nginx 배포 환경의 호출 경로를 맞춘다.

주의:

- 도메인 API를 SvelteKit `+server.ts`로 재구현하지 않는다.
- 이 route는 proxy boundary다.

### `/dd/[...path]`

파일:

- `apps/web/src/routes/dd/[...path]/+server.ts`

목적:

- Data Dragon 이미지/asset proxy.
- champion icon, spell icon, item image 등 브라우저 표시용.
- 이미지 호출량이 많으므로 browser/nginx cache를 적극 활용한다.

캐시:

```txt
/dd/cdn/{version}/...
  -> Cache-Control: public, max-age=31536000, immutable

/dd/api/versions.json 또는 latest 조회
  -> short TTL
```

원칙:

- image binary를 Valkey에 저장하지 않는다.
- versioned Data Dragon asset은 HTTP cache 계층에서 처리한다.

## 일반 내전 Routes

### `/recruitments`

파일:

- `apps/web/src/routes/recruitments/+page.svelte`

목적:

- 일반 모집 목록.
- OPEN/CLOSED/CANCELLED 등 상태별 필터.
- 엔트리 편성으로 이동.

Backend 데이터:

- `GET /api/recruitments`
- `GET /api/me`

WebSocket:

- `dashboard`

### `/recruitments/[id]/entry`

파일:

- `apps/web/src/routes/recruitments/[id]/entry/+page.svelte`

목적:

- 일반 내전 엔트리 편성.
- 참가자 팀/라인 배치.
- draft autosave.
- 모집 reopen.
- series 생성.

Bot 딥링크:

- Discord 일반 모집 메시지의 `엔트리 수정 시작` 액션에서 이 route로 이동한다.

Backend 데이터:

- `GET /api/recruitments/:id`
- `PUT /api/recruitments/:id/entry-draft`
- `POST /api/recruitments/:id/reopen`
- `POST /api/series`

WebSocket:

- `recruitment:{id}`

### `/series`

파일:

- `apps/web/src/routes/series/+page.svelte`

목적:

- 진행 중 또는 운영 가능한 일반 내전 시리즈 목록.
- pick-ban 또는 result route로 이동한다.

Backend 데이터:

- `GET /api/series`

WebSocket:

- `dashboard`

### `/series/completed`

파일:

- `apps/web/src/routes/series/completed/+page.svelte`

목적:

- 완료된 일반 내전 목록.
- 결과 조회 route로 이동한다.

Backend 데이터:

- `GET /api/series/completed`

### `/series/[id]/pick-ban`

파일:

- `apps/web/src/routes/series/[id]/pick-ban/+page.svelte`

목적:

- 일반 내전 픽/밴 draft.
- side 선택, game tab, 밴/픽 입력, Hard Fearless 반영.
- 게임 결과 입력.
- 마지막 게임 되돌리기.
- 정해진 지점 되돌리기.

Bot 딥링크:

- series 생성 후 bot follow-up 또는 dashboard에서 진입한다.

Backend 데이터:

- `GET /api/series/:id`
- `PUT /api/series/:id/pickban`
- `POST /api/series/:id/games`
- `DELETE /api/series/:id/games/last`
- `POST /api/series/:id/revert`
- `GET /api/guilds/:guildId/reverts/recent`
- `POST /api/guilds/:guildId/reverts/apply-last`
- `GET /api/champions`

WebSocket:

- `series:{id}`

### `/series/[id]/result`

파일:

- `apps/web/src/routes/series/[id]/result/+page.svelte`

목적:

- 일반 내전 최종 결과.
- 게임별 pick/ban, 결과, MMR 변화, 참가자 기록 표시.

Backend 데이터:

- `GET /api/series/:id`
- `GET /api/series/:id/balance.svg`

WebSocket:

- `series:{id}`

## 경매 내전 Routes

### `/auction`

파일:

- `apps/web/src/routes/auction/+page.svelte`

목적:

- 경매 모집, 진행 중 경매 토너먼트, 완료/취소 경매 진입점.

Backend 데이터:

- `GET /api/auction-recruitments`
- `GET /api/auction-tournaments`

WebSocket:

- `auction-dashboard`

### `/auction/recruitments/[id]/draft`

파일:

- `apps/web/src/routes/auction/recruitments/[id]/draft/+page.svelte`

목적:

- 경매 모집에서 토너먼트를 시작하는 화면.
- 참가자 확인, 팀장 선출 단계로 전환.

Bot 딥링크:

- Discord 경매 모집 메시지의 `경매 시작` 액션에서 이 route로 이동한다.

Backend 데이터:

- `GET /api/auction-recruitments/:id`
- `POST /api/auction-tournaments`

WebSocket:

- `auction-recruitment:{id}`

### `/auction/tournaments/[id]/draft`

파일:

- `apps/web/src/routes/auction/tournaments/[id]/draft/+page.svelte`

목적:

- 경매 토너먼트 드래프트.
- 팀장 선택, 포인트 배정, 후보 draw, bid intent, 낙찰, 수동 배치, 유찰, 단계 되돌리기.
- 최근 입찰/수동 배치 단계 되돌리기.

Backend 데이터:

- `GET /api/auction-tournaments/:id`
- `PUT /api/auction-tournaments/:id/captains`
- `PUT /api/auction-tournaments/:id/points`
- `POST /api/auction-tournaments/:id/start-bidding`
- `POST /api/auction-tournaments/:id/draw`
- `POST /api/auction-tournaments/:id/finalize-bid`
- `POST /api/auction-tournaments/:id/manual-assign`
- `POST /api/auction-tournaments/:id/cancel-draw`
- `POST /api/auction-tournaments/:id/bid-intent`
- `POST /api/auction-tournaments/:id/revert-bid`
- `POST /api/auction-tournaments/:id/revert-stage`
- `POST /api/auction-tournaments/:id/start-bracket`
- `POST /api/auction-tournaments/:id/cancel`
- `GET /api/guilds/:guildId/reverts/recent`
- `POST /api/guilds/:guildId/reverts/apply-last`

WebSocket:

- `auction-tournament:{id}`

### `/auction/tournaments/[id]/bracket`

파일:

- `apps/web/src/routes/auction/tournaments/[id]/bracket/+page.svelte`

목적:

- 경매 브래킷, match별 라인업 입력, match game result 입력.
- BO1/BO3 포맷, match 생성, 양 팀 5인 라인업 확정, match game result 입력, 마지막 게임 삭제.
- 게임 시작 전 TOP/JUNGLE/MID/ADC/SUPPORT 라인업 입력을 반드시 지원한다.
- 라인업 확정 취소와 match result 되돌리기를 지원한다.
- 브래킷 화면은 운영자용 readiness/control dashboard를 별도로 두지 않는다.
- match 상태 표시는 `AuctionMatchSummaryCard` 같은 compact 진입 카드 안에서만 최소한으로 제공한다.
- match game result 입력은 별도 경매 전용 결과 입력 컴포넌트를 새로 만들지 않고 기존 game result 입력 흐름을 사용한다.

컴포넌트 계획:

- 완료: `AuctionTournamentBracketMatchSetupBoard`
- 완료: `AuctionMatchStartingLineupInputBoard`
- 다음: `AuctionMatchSummaryCard`

Backend 데이터:

- `GET /api/auction-tournaments/:id`
- `GET /api/auction-matches/:matchId`
- `POST /api/auction-tournaments/:id/matches`
- `PUT /api/auction-matches/:matchId/format`
- `PUT /api/auction-matches/:matchId/lineup`
- `POST /api/auction-matches/:matchId/lineup/lock`
- `POST /api/auction-matches/:matchId/games`
- `DELETE /api/auction-matches/:matchId/games/last`
- `GET /api/guilds/:guildId/reverts/recent`
- `POST /api/guilds/:guildId/reverts/apply-last`
- `GET /api/champions`

불변 조건:

- lineups locked 전에는 `POST /api/auction-matches/:matchId/games`를 호출할 수 없다.
- 각 팀은 TOP/JUNGLE/MID/ADC/SUPPORT가 정확히 1명씩 있어야 한다.

WebSocket:

- `auction-tournament:{id}`
- `auction-match:{matchId}`

### `/auction/tournaments/[id]/result`

파일:

- `apps/web/src/routes/auction/tournaments/[id]/result/+page.svelte`

목적:

- 경매 토너먼트 결과.
- 우승 팀, match 결과, game ledger, pick/ban 기록 표시.
- 경매 최종 결과는 self-service 입력 완료 후 참가자가 확인하는 화면으로 둔다.
- 운영자용 상태 대시보드가 아니라 우승팀, 팀 구성, 낙찰 기록, match 스코어를 읽기 쉽게 정리한다.

컴포넌트 계획:

- 다음: `AuctionTournamentResultBoard`
- 다음: `AuctionPlayerLedgerBoard`

Backend 데이터:

- `GET /api/auction-tournaments/:id`

WebSocket:

- `auction-tournament:{id}`

## 사용자 / 통계 Routes

### `/g/[guildId]/records`

파일:

- `apps/web/src/routes/g/[guildId]/records/+page.svelte`

목적:

- 일반적인 전적 검색 진입점.
- Discord 사용자, Riot ID, guild 표시 이름으로 검색한다.
- 내부 내전 전적을 먼저 보여주고 Riot 외부 전적은 보조 정보로 붙인다.

Backend 데이터:

- `GET /api/guilds/:guildId/records/search`
- `GET /api/guilds/:guildId/riot/lookup`
- `GET /api/guilds/:guildId/riot/recent-matches`

주의:

- 내부 내전 기록은 반드시 현재 guild의 `games` 원장만 조회한다.
- 현재 사용자가 guild 소속이 아니면 `guilds.is_externally_public`이 true인 공개 범위만 보여준다.
- 공개 범위에는 운영 화면, 감사 로그, 멤버 관리, 권한 정보가 포함되지 않는다.

### `/users`

파일:

- `apps/web/src/routes/users/+page.svelte`

목적:

- 사용자 검색.
- Discord 사용자, Riot ID, 최근 운영 기록으로 profile 또는 guild records route 진입.

Backend 데이터:

- `GET /api/users/search`
- `GET /api/me/guilds`
- `PUT /api/me/guild-preferences`

주의:

- 사용자가 속한 guild 선택, 즐겨찾기, 정렬 순서는 `user_guild_preferences`를 기준으로 유지한다.
- Custom 전적 검색에서 어떤 guild의 기록을 볼지는 검색 결과 화면에서 사용자가 선택한다.

### `/users/[discordId]`

파일:

- `apps/web/src/routes/users/[discordId]/+page.svelte`

목적:

- 사용자 프로필.
- 라인별 MMR, 최근 내전 전적, 챔피언 선호, 경매 카드, screening report.

Backend 데이터:

- `GET /api/users/:id/profile`
- `GET /api/users/:id/records`
- `GET /api/users/:id/mmr-history`
- `GET /api/users/:id/preferences`
- `GET /api/users/:id/auction-card`
- `GET /api/users/:id/screening-report`

WebSocket:

- `user:{discordId}`

### `/leaderboard`

파일:

- `apps/web/src/routes/leaderboard/+page.svelte`

목적:

- 라인별/통합 리더보드.

Backend 데이터:

- `GET /api/leaderboard`
- `GET /api/leaderboard/composite`

WebSocket:

- `leaderboard:{role}`
- `leaderboard:COMPOSITE`

### `/me/riot-accounts`

파일:

- `apps/web/src/routes/me/riot-accounts/+page.svelte`

목적:

- 내 Riot 계정 연결/삭제/메인 지정/갱신.

Backend 데이터:

- `GET /api/me/riot-accounts`
- `POST /api/me/riot-accounts`
- `DELETE /api/me/riot-accounts/:puuid`
- `PUT /api/me/riot-accounts/:puuid/main`
- `POST /api/me/riot-accounts/:puuid/refresh`

WebSocket:

- `user:{currentDiscordId}`

## 도구 Routes

### `/tools/minigame`

파일:

- `apps/web/src/routes/tools/minigame/+page.svelte`

목적:

- 룰렛, 사다리, 코인플립.
- 내전 운영 중 보조 도구.

데이터:

- 기본적으로 local UI state.
- 필요 시 참가자 목록을 query 기반으로 받아올 수 있다.

우선순위:

- 1차 MVP에는 포함하지 않아도 된다.

## Admin Routes

### `/g/[guildId]/admin/logs`

파일:

- `apps/web/src/routes/g/[guildId]/admin/logs/+page.svelte`

목적:

- audit log 조회.
- destructive/admin action 추적.
- 최근 revert checkpoint 조회와 되돌리기.

Backend 데이터:

- `GET /api/guilds/:guildId/reverts/recent`
- `POST /api/guilds/:guildId/reverts/:checkpointId/apply`
- `GET /api/logs`
- `GET /api/logs/data`
- `POST /api/logs/logout`

권한:

- operator/admin.

### `/admin/perms`

파일:

- `apps/web/src/routes/admin/perms/+page.svelte`

목적:

- 현재 Discord session의 권한 진단.
- guild/role/operator permission 문제 확인.

Backend 데이터:

- `GET /api/me/perms`

권한:

- 로그인 사용자.

### `/admin/status`

파일:

- `apps/web/src/routes/admin/status/+page.svelte`

목적:

- API, bot heartbeat, DB, KV, Data Dragon, internal notify 상태 확인.

Backend 데이터:

- `GET /api/healthz/deep`
- 필요 시 admin status 전용 endpoint.

권한:

- operator/admin.

## Frontend 우선순위

### P0

- `/`
- `/oauth/[...path]`
- `/api/[...path]`
- `/dd/[...path]`
- `/recruitments`
- `/recruitments/[id]/entry`
- `/series/[id]/pick-ban`
- `/series/[id]/result`

### P1

- `/series`
- `/series/completed`
- `/auction`
- `/auction/recruitments/[id]/draft`
- `/auction/tournaments/[id]/draft`
- `/auction/tournaments/[id]/bracket`
- `/auction/tournaments/[id]/result`
- `/users`
- `/users/[discordId]`
- `/leaderboard`
- `/me/riot-accounts`

### P2

- `/admin/logs`
- `/admin/status`
- `/admin/perms`
- `/tools/minigame`

## 만들지 않을 Frontend Route

- `/activity/*`: Discord Activity route는 만들지 않는다.
- `/login`: `/oauth/discord/start` 링크 또는 button으로 충분하면 별도 로그인 페이지를 만들지 않는다. 만들더라도 Discord OAuth2 시작 버튼만 둔다.
- `/signup`: 별도 회원가입은 절대 만들지 않는다.
- `/register`: 앱 회원가입 route는 만들지 않는다. Riot ID 연결은 `/me/riot-accounts` 또는 Discord `/등록`에서 처리한다.
- `/password/*`: password reset/change flow는 만들지 않는다.
- `/settings`: 사용자/관리자 설정 요구가 구체화될 때 만든다.
- `/reports`: 통계 report 제품 요구가 생기기 전까지 만들지 않는다.
