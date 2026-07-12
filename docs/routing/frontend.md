# Frontend Routing

작성일: 2026-07-12

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

Workflow root는 API의 UUID `id`와 사람용 `displayNumber`를 구분한다. URL/deep link에는 `id`를
사용하고 header, `#214`, `A-118` 같은 표시는 `displayNumber`를 사용한다. 현재 mock route ID에
포함된 숫자를 production label로 재사용하지 않는다.

인증 UI 정책:

- 로그인 시작은 Discord OAuth2로만 제공한다.
- 별도 회원가입 화면은 만들지 않는다.
- 이메일/비밀번호 로그인 화면은 만들지 않는다.
- 계정 생성 form, password reset form, email verification flow는 만들지 않는다.
- Riot 계정 연결 화면은 회원가입이 아니라 Discord 로그인 이후의 프로필 연결 기능이다.

## 실시간 편집 UI 규칙

WebSocket을 사용하는 작업 화면은 현재 component 계약처럼 상태를 두 축으로 제공한다.

- connection: `connected`, `reconnecting`, `offline`.
- save: `saved`, `saving`, `dirty`, `conflict`.
- `loading`은 route/snapshot fetch 상태로 별도 처리한다.
- 연결과 저장 상태는 `RealtimeEditStateBar`로 표시한다.
- 저장하지 않은 local 입력을 invalidate만으로 덮어쓰지 않는다.
- 위험 command는 최신 revision을 확인한 뒤 실행한다.
- reconnect 후 topic 재구독과 aggregate 전체 재조회를 수행한다.

화면별 적용:

```txt
/g/[guildId]/recruitments/[id]/entry       entry draft autosave + conflict notice
/g/[guildId]/series/[id]/pick-ban          pick/ban autosave + game result command guard
/g/[guildId]/auction/recruitments/[id]/draft captain/point setup revision + conflict notice
/g/[guildId]/auction/tournaments/[id]/draft live auction revision + guarded command
/g/[guildId]/auction/tournaments/[id]/bracket lineup/result revision + command guard
```

상태 전이, autosave, 충돌 처리와 재연결 계약은 [Realtime Collaboration](../07-realtime-collaboration.md)을 따른다.

## App Shell

### `/`

파일:

- `apps/web/src/routes/+page.svelte`

목적:

- 로그인 사용자의 대표 guild를 확인해 `/g/[guildId]`로 redirect한다.
- 대표 guild가 없으면 `/me?tab=guild`의 설정 흐름으로 보낸다.

Backend 데이터:

- `GET /api/me`
- `GET /api/me/guilds`

### `/g/[guildId]`

파일:

- `apps/web/src/routes/g/[guildId]/+page.svelte`

목적:

- 현재 guild에서 입력해야 하는 편성, 픽밴, 결과, 경매 작업을 우선순위 순으로 확인한다.
- guild 전적 화면과 각 작업 route로 바로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/operations`

WebSocket: 사용하지 않는다.

### `+layout.svelte`

파일:

- `apps/web/src/routes/+layout.svelte`

목적:

- 공통 shell, navigation, auth bootstrap, toast/overlay root.
- 로그인 상태와 현재 권한 요약을 표시한다.
- 오른쪽 guild selector가 현재 `guildId` context를 표시하고 서버 전환 시 해당 guild dashboard로 이동한다.
- 모집 마감 상태는 `내전` 메뉴에 포함하고, `전적`, `랭킹`, `관리`는 현재 guild 기준 route를 사용한다.
- `관리`는 현재 guild의 operator에게만 노출하고 `/g/[guildId]/admin`으로 이동한다.
- `QuickActionFab`을 전역 배치해 미니게임과 타이머를 연다.

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
- 현재 proxy는 `/api/` prefix를 제거한 path를 `API_BASE_URL`에 붙이므로 local/dev 값은 `http://backend:port/api/`처럼 `/api/` suffix를 포함한다.
- nginx가 browser의 `/api/`를 Axum으로 직접 전달하는 배포 경로와 최종 upstream path가 같아야 한다.

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

### `/g/[guildId]/recruitments/[id]/entry`

파일:

- `apps/web/src/routes/g/[guildId]/recruitments/[id]/entry/+page.svelte`

목적:

- 일반 내전 엔트리 편성.
- 참가자 팀/라인 배치.
- draft autosave.
- 모집 reopen.
- series 생성. 현재 format control이 없으므로 신규 series는 BO3다.

Bot 딥링크:

- Discord 일반 모집 메시지의 `엔트리 수정 시작` 액션에서 이 route로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/recruitments/:id`
- `PATCH /api/guilds/:guildId/recruitments/:id/entry-draft`
- `POST /api/guilds/:guildId/recruitments/:id/entry-draft/reset`
- `POST /api/guilds/:guildId/recruitments/:id/reopen`
- `POST /api/guilds/:guildId/series`

현재 mock page에 없는 reset/reopen/createSeries control은 API adapter 단계에서
`EntryDraftBoard` callback으로 연결해야 하며 backend만 구현하고 숨겨 두지 않는다.

WebSocket:

- `guild:{guildId}:recruitment:{id}`

### `/g/[guildId]/series`

파일:

- `apps/web/src/routes/g/[guildId]/series/+page.svelte`

목적:

- 상단에는 모집 마감, 엔트리 편성, 픽밴, 결과 입력 상태를 일반 내전 작업 목록으로 보여준다.
- 하단에는 완료된 일반 내전을 최신 순으로 보여준다.
- pick-ban 또는 result route로 이동한다.
- `RESULT`처럼 아직 game 입력이 필요한 작업은 pick-ban route로 이동하고, 완료된 row만 result
  route로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/recruitments?state=actionable`
- `GET /api/guilds/:guildId/series?state=active`
- `GET /api/guilds/:guildId/series?state=completed`

WebSocket: 사용하지 않는다.

### `/g/[guildId]/series/completed` 호환 경로

파일:

- `apps/web/src/routes/g/[guildId]/series/completed/+page.svelte`

목적:

- `/g/[guildId]/series#completed`로 redirect한다.
- 기존 Bot 메시지와 북마크 호환만 유지한다.

Backend 데이터:

- `GET /api/guilds/:guildId/series?state=completed`

### `/g/[guildId]/series/[id]/pick-ban`

파일:

- `apps/web/src/routes/g/[guildId]/series/[id]/pick-ban/+page.svelte`

목적:

- 일반 내전 픽/밴 draft.
- side 선택, game tab, 선택적 밴/픽, 승자 입력.
- 게임 결과 입력.
- 마지막 게임 되돌리기.
- 정해진 지점 되돌리기.

Bot 딥링크:

- series 생성 후 bot follow-up 또는 dashboard에서 진입한다.

Backend 데이터:

- `GET /api/guilds/:guildId/series/:id`
- `PATCH /api/guilds/:guildId/series/:id/pickban`
- `POST /api/guilds/:guildId/series/:id/games`
- `DELETE /api/guilds/:guildId/series/:id/games/last`
- `POST /api/guilds/:guildId/series/:id/revert`

현재 mock page에 연결되지 않은 submit/deleteLast/revert는 API adapter 단계에서
`ScrimPickBanResultInputBoard` control로 연결한다.

WebSocket:

- `guild:{guildId}:series:{id}`

### `/g/[guildId]/series/[id]/result`

파일:

- `apps/web/src/routes/g/[guildId]/series/[id]/result/+page.svelte`

목적:

- 일반 내전 최종 결과.
- 게임별 pick/ban, 결과, MMR 변화, 참가자 기록 표시.
- `durationSeconds`가 null이면 `SeriesResultLedgerBoard` adapter가 게임 시간을 `-`로 표시한다.

Backend 데이터:

- `GET /api/guilds/:guildId/series/:id`
- `GET /api/guilds/:guildId/series/:id/balance.svg`

WebSocket: 읽기 전용 결과 화면에서는 사용하지 않는다.

## 경매 내전 Routes

### `/g/[guildId]/auction`

파일:

- `apps/web/src/routes/g/[guildId]/auction/+page.svelte`

목적:

- 상단에는 경매 모집과 진행 중 경매 토너먼트, 하단에는 완료된 경매를 표시한다.
- 팀장 설정, 입찰 진행, 브래킷 진행, 결과 입력, 결과 확정 상태를 구분한다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-recruitments?state=actionable`
- `GET /api/guilds/:guildId/auction-tournaments?state=active`
- `GET /api/guilds/:guildId/auction-tournaments?state=completed`

WebSocket: 사용하지 않는다.

### `/g/[guildId]/auction/recruitments/[id]/draft`

파일:

- `apps/web/src/routes/g/[guildId]/auction/recruitments/[id]/draft/+page.svelte`

목적:

- 경매 모집에서 토너먼트를 시작하는 화면.
- 참가자 확인, 팀장 선출 단계로 전환.

Bot 딥링크:

- Discord 경매 모집 메시지의 `경매 시작` 액션에서 이 route로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-recruitments/:id`
- `PATCH /api/guilds/:guildId/auction-recruitments/:id/setup-draft`
- `POST /api/guilds/:guildId/auction-tournaments`

WebSocket:

- `guild:{guildId}:auction-recruitment:{id}`

### `/g/[guildId]/auction/tournaments/[id]/draft`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/draft/+page.svelte`

목적:

- 경매 토너먼트 드래프트.
- 후보 draw, 낙찰, 수동 배치, 유찰, 복구, 최근 단계 되돌리기.
- 최근 입찰/수동 배치 단계 되돌리기.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id`
- `POST /api/guilds/:guildId/auction-tournaments/:id/commands`

WebSocket:

- `guild:{guildId}:auction-tournament:{id}`

### `/g/[guildId]/auction/tournaments/[id]/bracket`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/bracket/+page.svelte`

목적:

- 경매 브래킷, match별 라인업 입력, match game result 입력.
- `?view=result`로 진입하면 `AuctionTournamentBracketResultBoard` 결과 입력 탭을 바로 연다.
- 신규 match는 BO3이며 저장된 legacy BO1/BO3와 향후 BO5는 read DTO에서 그대로 표시한다. 현재
  MVP에는 format 변경 control을 만들지 않는다.
- match 생성, 양 팀 5인 라인업 확정, match game result 입력, 마지막 게임 삭제.
- 게임 시작 전 TOP/JUNGLE/MID/BOTTOM/SUPPORT 라인업 입력을 반드시 지원한다.
- 라인업 확정 취소와 match result 되돌리기는 길드 관리의 checkpoint 복구를 사용한다. bracket
  화면에 별도 unlock command는 현재 제공하지 않는다.
- 브래킷 화면은 운영자용 readiness/control dashboard를 별도로 두지 않는다.
- match 상태 표시는 `AuctionMatchSummaryCard` 같은 compact 진입 카드 안에서만 최소한으로 제공한다.
- match game result 입력은 별도 경매 전용 결과 입력 컴포넌트를 새로 만들지 않고 기존 game result 입력 흐름을 사용한다.

컴포넌트 계획:

- 완료: `AuctionTournamentBracketMatchSetupBoard`
- 완료: `AuctionMatchStartingLineupInputBoard`
- 다음: `AuctionMatchSummaryCard`

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id`
- `POST /api/guilds/:guildId/auction-tournaments/:id/bracket/commands`
- `GET /api/guilds/:guildId/auction-matches/:matchId`
- `PATCH /api/guilds/:guildId/auction-matches/:matchId/lineup`
- `POST /api/guilds/:guildId/auction-matches/:matchId/lineup/lock`
- `POST /api/guilds/:guildId/auction-matches/:matchId/games`
- `DELETE /api/guilds/:guildId/auction-matches/:matchId/games/last`

불변 조건:

- lineups locked 전에는 `POST /api/guilds/:guildId/auction-matches/:matchId/games`를 호출할 수 없다.
- 각 팀은 TOP/JUNGLE/MID/BOTTOM/SUPPORT가 정확히 1명씩 있어야 한다.

WebSocket:

- `guild:{guildId}:auction-tournament:{id}`
- `guild:{guildId}:auction-match:{matchId}`

### `/g/[guildId]/auction/tournaments/[id]/result`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/result/+page.svelte`

목적:

- 경매 토너먼트 결과.
- 우승 팀, match 결과, game ledger, pick/ban 기록 표시.
- 경매 진행 요약은 self-service 입력 완료 후 참가자가 확인하는 원장으로 둔다.
- 운영자용 control dashboard가 아니라 팀 구성, 포인트 정산, MMR 변화, match 스코어를 읽기 쉽게 정리한다.
- 예외적으로 operator에게만 `결과 입력 되돌리기` 단일 command를 제공한다.

컴포넌트 계획:

- 완료: `AuctionTournamentOverviewBoard`
- 통합: `AuctionPlayerLedgerBoard` 역할은 overview board의 참가자 정산 섹션에 포함한다.
- 연결 필요: `onRevert(checkpointId, expectedRevision)` callback.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id/result`
- `POST /api/guilds/:guildId/reverts/:checkpointId/apply`

WebSocket은 사용하지 않는다. Revert 성공 후 HTTP response/snapshot으로 이동하거나 갱신한다.

## 사용자 / 통계 Routes

### `/g/[guildId]/records`

파일:

- `apps/web/src/routes/g/[guildId]/records/+page.svelte`

목적:

- 현재 guild의 내부 내전 전적 검색 진입점.
- Discord 사용자, 연결 Riot ID, guild 표시 이름으로 사용자를 찾는다.

Backend 데이터:

- `GET /api/guilds/:guildId/records/search`

주의:

- 내부 내전 기록은 반드시 현재 guild의 `games` 원장만 조회한다.
- 현재 사용자가 guild 소속이 아니면 `guild_config.is_externally_public`이 true인 공개 범위만 보여준다.
- 공개 범위에는 운영 화면, 감사 로그, 멤버 관리, 권한 정보가 포함되지 않는다.

### `/records`

파일:

- `apps/web/src/routes/records/+page.svelte`

목적:

- navbar Riot ID 검색 결과를 표시한다.
- Riot profile/rank/champion aggregate/recent match와 10인 상세를 보여준다.
- guild 내부 custom match 원장과 분리한다.

Backend 데이터:

- `GET /api/riot/records?query=GameName%23TagLine&region=kr`
- `POST /api/riot/records/refresh`

WebSocket: 사용하지 않는다.

### `/users/[discordId]`

파일:

- `apps/web/src/routes/users/[discordId]/+page.svelte`

목적:

- 사용자 프로필.
- 현재 MVP에서는 표시 이름, Discord ID, 활동 guild 수, season 수를 보여준다.

Backend 데이터:

- `GET /api/users/:id/profile`

WebSocket: 사용하지 않는다.

### `/g/[guildId]/leaderboard`

파일:

- `apps/web/src/routes/g/[guildId]/leaderboard/+page.svelte`

목적:

- 라인별/통합 리더보드.

Backend 데이터:

- `GET /api/guilds/:guildId/leaderboard`

WebSocket: 사용하지 않는다.

### `/me`

파일:

- `apps/web/src/routes/me/+page.svelte`

목적:

- 선호 챔피언, Riot 계정, 대표 길드, 개인정보 설정을 하나의 내 정보 화면에서 관리한다.
- 개인정보 설정은 외부 프로필, 연결된 Riot ID, 외부 전적 검색 노출만 제어한다.
- 같은 guild 내부의 내전 기록과 MMR은 개인 설정으로 숨기지 않는다.

Backend 데이터:

- `GET /api/me/champion-preferences`
- `PUT /api/me/champion-preferences/:lane`
- `GET /api/me/guilds`
- `PUT /api/me/default-guild`
- `GET /api/me/riot-accounts`
- `POST /api/me/riot-accounts`
- `DELETE /api/me/riot-accounts/:puuid`
- `PUT /api/me/riot-accounts/:puuid/main`
- `POST /api/me/riot-accounts/:puuid/refresh`
- `GET /api/me/privacy`
- `PUT /api/me/privacy`

WebSocket: 사용하지 않는다.

## 도구 Routes

### `/docs`

파일:

- `apps/web/src/routes/docs/+page.svelte`
- `apps/web/src/lib/content/user-guide.md`

목적:

- 일반 사용자와 길드 운영자를 위한 정적 기능 설명.
- Markdown 원본을 build 시 렌더링하며 API나 사용자 상태를 요구하지 않는다.

### `/terms`

파일:

- `apps/web/src/routes/terms/+page.svelte`
- `apps/web/src/lib/content/terms.md`

목적:

- Discord Bot과 웹 콘솔 이용 조건을 공개하는 정적 이용약관.
- build 시 Markdown을 렌더링하고 Footer에서 접근한다.

### `/privacy`

파일:

- `apps/web/src/routes/privacy/+page.svelte`
- `apps/web/src/lib/content/privacy.md`

목적:

- 현재 구현된 Discord 인증, 길드 운영 데이터, 세션과 선택적 Riot 연동의 처리 범위를 공개한다.
- build 시 Markdown을 렌더링하고 Footer에서 접근한다.

`/docs`, `/terms`, `/privacy`는 `StaticMarkdownDocument`를 공유하며 API나 로그인 상태를
요구하지 않는다. 세 route에서는 운영용 `QuickActionFab`을 표시하지 않는다.

미니게임과 타이머는 독립 route를 만들지 않고 전역 `QuickActionFab` modal로만 제공한다.

## Admin Routes

### `/g/[guildId]/admin`

파일:

- `apps/web/src/routes/g/[guildId]/admin/+page.svelte`

목적:

- 길드 멤버 등록 상태, Riot 연결 상태, MMR을 조회하고 관리한다.
- 운영자 역할, 모집/로그 채널, 현재 시즌, 외부 공개 설정을 관리한다.
- 최근 revert checkpoint 조회와 고정 단계 되돌리기를 실행한다.
- audit log를 필터링하고 destructive/admin action을 추적한다.

탭:

- `members`: 멤버 검색, MMR 조정, 비활성화와 복구.
- `operations`: 최근 고정 checkpoint 조회와 복구.
- `settings`: Discord 역할/채널, 현재 시즌 확인과 공개 설정.
- `logs`: 작업, 대상, 실행자별 감사 로그와 상세 payload.

`mainLane`은 미등록/미설정 member에서 `null`일 수 있다. Adapter와
`GuildMemberManagementPanel`은 임의 lane을 합성하지 않고 `미설정`으로 표시한다.

호환 경로:

- `/g/[guildId]/admin/logs`는 `/g/[guildId]/admin?tab=logs`로 redirect한다.

Backend 데이터:

- `GET /api/guilds/:guildId/reverts/recent`
- `POST /api/guilds/:guildId/reverts/:checkpointId/apply`
- `PATCH /api/guilds/:guildId/members/:memberId/mmr`
- `PATCH /api/guilds/:guildId/members/:memberId/status`
- `PUT /api/guilds/:guildId/settings`
- `GET /api/guilds/:guildId/audit-logs`

권한:

- 현재 guild operator.

### `/admin/status`

파일:

- `apps/web/src/routes/admin/status/+page.svelte`

목적:

- API, PostgreSQL, Bot heartbeat, Valkey 상태 확인.

Backend 데이터:

- `GET /api/admin/status`

권한:

- service admin allowlist.

## Frontend 우선순위

### P0

- `/`
- `/oauth/[...path]`
- `/api/[...path]`
- `/dd/[...path]`
- `/g/[guildId]`
- `/g/[guildId]/recruitments/[id]/entry`
- `/g/[guildId]/series/[id]/pick-ban`
- `/g/[guildId]/series/[id]/result`

### P1

- `/g/[guildId]/series`
- `/g/[guildId]/auction`
- `/g/[guildId]/auction/recruitments/[id]/draft`
- `/g/[guildId]/auction/tournaments/[id]/draft`
- `/g/[guildId]/auction/tournaments/[id]/bracket`
- `/g/[guildId]/auction/tournaments/[id]/result`
- `/users/[discordId]`
- `/g/[guildId]/leaderboard`
- `/me`

### P2

- `/g/[guildId]/admin`
- `/admin/status`

## 만들지 않을 Frontend Route

- `/activity/*`: Discord Activity route는 만들지 않는다.
- `/login`: `/oauth/discord/start` 링크 또는 button으로 충분하면 별도 로그인 페이지를 만들지 않는다. 만들더라도 Discord OAuth2 시작 버튼만 둔다.
- `/signup`: 별도 회원가입은 절대 만들지 않는다.
- `/register`: 앱 회원가입 route는 만들지 않는다. Riot ID 연결은 `/me` 또는 Discord `/등록`에서 처리한다.
- `/password/*`: password reset/change flow는 만들지 않는다.
- `/settings`: 사용자/관리자 설정 요구가 구체화될 때 만든다.
- `/reports`: 통계 report 제품 요구가 생기기 전까지 만들지 않는다.
- `/users`: 검색은 `/g/[guildId]/records`에서 처리하고 사용자 상세만 `/users/[discordId]`로 연다.
- `/admin/perms`: 권한 실패는 해당 guild route의 403 화면에서 처리한다.
- `/tools/minigame`: 미니게임과 타이머는 `QuickActionFab` modal로 제공한다.
