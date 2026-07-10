# Backend Routing

작성일: 2026-07-10

## 범위

이 문서는 `apps/api`의 Fastify HTTP API, OAuth/session route, WebSocket route, internal route를 다룬다.

Backend route의 책임:

- 인증과 session 발급.
- operator/admin 권한 판단.
- DB read/write.
- domain state transition.
- audit log 기록.
- WebSocket topic invalidate.
- API -> Bot internal notify.

Backend route의 비책임:

- 화면 layout.
- Discord slash command parsing.
- Discord message component rendering.

## Auth / Session

인증 정책:

- Backend 인증은 오직 Discord OAuth2만 지원한다.
- 별도 회원가입 API를 만들지 않는다.
- 이메일/비밀번호 login API를 만들지 않는다.
- 자체 username/password credential을 저장하지 않는다.
- Discord OAuth2 callback에서 확인한 Discord user id를 session의 사용자 식별자로 사용한다.
- Riot 계정 연결은 인증 수단이 아니라 Discord 사용자에 연결되는 프로필 데이터다.

### `GET /oauth/discord/start`

목적:

- Discord OAuth authorize URL로 redirect한다.

사용자:

- Frontend login button.
- 권한이 필요한 route에서 login CTA.

### `GET /oauth/callback`

목적:

- Discord OAuth callback.
- state 검증.
- Discord token exchange.
- signed httpOnly `sid` cookie 발급.
- 필요 시 Discord user id 기준으로 내부 user record를 upsert한다. 이것은 별도 회원가입 플로우가 아니라 OAuth session bootstrap이다.

### `POST /api/logout`

목적:

- `sid` cookie clear.

### 만들지 않는 Auth API

다음 API는 만들지 않는다.

- `POST /api/signup`
- `POST /api/register`
- `POST /api/login` for username/password
- `POST /api/password/reset`
- `POST /api/email/verify`
- Discord 외 OAuth provider callback

### `GET /api/me`

목적:

- 현재 session 사용자와 기본 edit/operator 권한 조회.

Frontend 사용:

- root layout.
- dashboard.
- 권한 분기 UI.

### `GET /api/me/perms`

목적:

- 권한 진단.
- Discord guild/role/operator 상태 확인.

Frontend 사용:

- `/admin/perms`.

## 일반 내전 API

### `GET /api/recruitments`

목적:

- 일반 모집 목록 조회.

Frontend:

- `/`
- `/recruitments`

WebSocket invalidate:

- `dashboard`

### `GET /api/recruitments/:id`

목적:

- 일반 모집 상세 조회.
- 참가자, 라인 선호, entry draft 포함.

Frontend:

- `/recruitments/[id]/entry`

WebSocket invalidate:

- `recruitment:{id}`

### `PUT /api/recruitments/:id/entry-draft`

목적:

- 엔트리 편성 draft 저장.

Frontend:

- `/recruitments/[id]/entry`

권한:

- operator.

### `POST /api/recruitments/:id/reopen`

목적:

- CLOSED 모집을 OPEN 상태로 복구.

Frontend:

- `/recruitments/[id]/entry`

권한:

- operator.

Audit:

- 기록 필요.

### `POST /api/series`

목적:

- recruitment를 series로 전환.
- `series.id === recruitments.id` 유지.

Frontend:

- `/recruitments/[id]/entry`

Bot notify:

- `/internal/series-created`

WebSocket invalidate:

- `dashboard`
- `recruitment:{id}`
- `series:{id}`

### `GET /api/series`

목적:

- 진행 중 또는 운영 가능한 일반 내전 시리즈 목록.

Frontend:

- `/`
- `/series`

### `GET /api/series/completed`

목적:

- 완료된 일반 내전 목록.

Frontend:

- `/`
- `/series/completed`

### `GET /api/series/:id`

목적:

- 일반 내전 상세 조회.
- 참가자, 게임, 픽/밴, MMR change 포함.

Frontend:

- `/series/[id]/pick-ban`
- `/series/[id]/result`

WebSocket invalidate:

- `series:{id}`

### `PUT /api/series/:id/pickban`

목적:

- pick/ban draft 저장.

Frontend:

- `/series/[id]/pick-ban`

권한:

- operator.

### `POST /api/series/:id/games`

목적:

- 일반 내전 게임 결과 입력.
- `games`, `game_stats`, `game_picks`, `game_bans`, `mmr_changes`, `user_lane_mmr` 갱신.
- 일반 내전은 MMR에 반영한다.

Frontend:

- `/series/[id]/pick-ban`

Bot notify:

- series 완료 시 `/internal/series-completed`

### `DELETE /api/series/:id/games/last`

목적:

- 마지막 게임 결과 삭제/되돌리기.
- MMR rollback 포함.

Frontend:

- `/series/[id]/pick-ban`

권한:

- operator.

Audit:

- 기록 필요.

### `POST /api/series/:id/revert`

목적:

- series 상태 또는 최근 작업 되돌리기.

Frontend:

- `/series/[id]/pick-ban`

권한:

- operator.

Audit:

- 기록 필요.

### `GET /api/series/:id/balance.svg`

목적:

- 밸런스/라인업 SVG 생성.

Frontend:

- `/series/[id]/result`

Bot:

- 라인업 이미지/종료 카드 발행 시 참고 가능.

## 경매 내전 API

### `GET /api/auction-recruitments`

목적:

- 경매 모집 목록.

Frontend:

- `/`
- `/auction`

### `GET /api/auction-recruitments/:id`

목적:

- 경매 모집 상세 조회.

Frontend:

- `/auction/recruitments/[id]/draft`

### `POST /api/auction-recruitments/:id/members`

목적:

- 경매 모집 강제 참가.

Bot:

- `/경매인원추가`.

권한:

- operator.

### `DELETE /api/auction-recruitments/:id/members/:uid`

목적:

- 경매 모집 강제 제거.

Bot:

- `/경매인원삭제`.

권한:

- operator.

### `POST /api/auction-recruitments/:id/cancel`

목적:

- 경매 모집 취소.

Bot/Frontend:

- 경매 모집 취소 플로우.

Audit:

- 기록 필요.

### `POST /api/auction-tournaments`

목적:

- auction recruitment를 tournament로 전환.
- `auction_tournaments.id === auction_recruitments.id` 유지.

Frontend:

- `/auction/recruitments/[id]/draft`

WebSocket invalidate:

- `auction-dashboard`
- `auction-recruitment:{id}`
- `auction-tournament:{id}`

### `GET /api/auction-tournaments/:id`

목적:

- 경매 토너먼트 상세 조회.

Frontend:

- `/auction/tournaments/[id]/draft`
- `/auction/tournaments/[id]/bracket`
- `/auction/tournaments/[id]/result`

### 경매 draft mutation

Routes:

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

목적:

- 팀장, 포인트, 입찰, 수동 배치, 되돌리기, 브래킷 시작, 취소.

Frontend:

- `/auction/tournaments/[id]/draft`

권한:

- operator.

WebSocket invalidate:

- `auction-tournament:{id}`

Audit:

- destructive/revert/cancel 계열은 기록 필요.

### `GET /api/auction-matches/:matchId`

목적:

- 경매 match 상세 조회.

Frontend:

- `/auction/tournaments/[id]/bracket`

### `POST /api/auction-tournaments/:id/matches`

목적:

- 경매 match 생성.

Frontend:

- `/auction/tournaments/[id]/bracket`

### `PUT /api/auction-matches/:matchId/format`

목적:

- BO1/BO3 등 match format 변경.

Frontend:

- `/auction/tournaments/[id]/bracket`

### `PUT /api/auction-matches/:matchId/lineup`

목적:

- 경매 match 시작 전 양 팀 라인업 draft 저장.
- 각 팀 5명의 TOP/JUNGLE/MID/ADC/SUPPORT 배정을 기록.

Frontend:

- `/auction/tournaments/[id]/bracket`

권한:

- operator.

### `POST /api/auction-matches/:matchId/lineup/lock`

목적:

- 경매 match 라인업 확정.
- 양 팀 모두 TOP/JUNGLE/MID/ADC/SUPPORT가 정확히 1명씩 있어야 한다.
- 라인업 확정 이후에만 game result 입력이 가능하다.

Frontend:

- `/auction/tournaments/[id]/bracket`

권한:

- operator.

Audit:

- 기록 필요.

### `POST /api/auction-matches/:matchId/games`

목적:

- 경매 match 게임 결과 입력.
- lineup locked 상태에서만 허용.
- 경매는 MMR에 반영하지 않는다.

Frontend:

- `/auction/tournaments/[id]/bracket`

Bot notify:

- match 생성/완료/토너먼트 완료 알림.

### `DELETE /api/auction-matches/:matchId/games/last`

목적:

- 경매 match 마지막 게임 삭제.

Frontend:

- `/auction/tournaments/[id]/bracket`

Audit:

- 기록 필요.

## 사용자 / Riot / 통계 API

Routes:

- `GET /api/guilds/:guildId/records/search`
- `GET /api/guilds/:guildId/users/:id/records`
- `GET /api/guilds/:guildId/riot/lookup`
- `GET /api/guilds/:guildId/riot/recent-matches`
- `GET /api/users/search`
- `GET /api/users/:id/profile`
- `GET /api/users/:id/auction-card`
- `GET /api/users/:id/mmr-history`
- `GET /api/users/:id/preferences`
- `PUT /api/users/me/preferences`
- `GET /api/me/guilds`
- `PUT /api/me/guild-preferences`
- `GET /api/me/riot-accounts`
- `POST /api/me/riot-accounts`
- `DELETE /api/me/riot-accounts/:puuid`
- `PUT /api/me/riot-accounts/:puuid/main`
- `POST /api/me/riot-accounts/:puuid/refresh`
- `GET /api/leaderboard`
- `GET /api/leaderboard/composite`
- `GET /api/champions`
- `GET /api/screening/lol/:region/:gameName/:tagLine`
- `GET /api/users/:id/screening-report`

Frontend:

- `/g/[guildId]/records`
- `/users`
- `/users/[discordId]`
- `/leaderboard`
- `/me/riot-accounts`
- `/series/[id]/pick-ban`
- `/auction/tournaments/[id]/bracket`

권한/공개 범위:

- guild 소속 사용자는 해당 guild의 내부 전적과 운영 화면을 권한에 따라 조회한다.
- guild 소속이 아닌 사용자는 `guilds.is_externally_public = true`인 guild의 공개 화면/공개 전적만 조회할 수 있다.
- `GET /api/me/guilds`는 Discord OAuth2 사용자 기준으로 소속 guild와 `user_guild_preferences`를 함께 반환한다.
- `PUT /api/me/guild-preferences`는 사용자의 최근 선택 guild, 즐겨찾기, 정렬 순서를 저장한다.

Bot:

- `/등록`
- `/내정보`
- `/내정보갱신`
- `/내전기록`
- `/랭킹`
- `/전적`
- `/지금게임`
- `/전적검토`

구조:

```txt
internal record search
  /api/guilds/:guildId/records/search
  /api/guilds/:guildId/users/:id/records
  -> users / guild_members / riot_accounts
  -> games / game_stats / game_picks / game_bans / mmr_changes

external Riot lookup
  /api/guilds/:guildId/riot/lookup
  /api/guilds/:guildId/riot/recent-matches
  -> Riot API
  -> guild_kv cache
```

원칙:

- 내부 내전 전적은 `games.guild_id`를 기준으로만 조회한다.
- Riot 외부 전적은 내부 MMR 계산의 source of truth가 아니다.
- `/전적`과 웹 전적 검색은 같은 read service를 사용한다.

## Admin / 운영 API

Routes:

- `GET /healthz`
- `GET /api/healthz/deep`
- `GET /api/guilds/:guildId/reverts/recent`
- `POST /api/guilds/:guildId/reverts/:checkpointId/apply`
- `POST /api/guilds/:guildId/reverts/apply-last`
- `GET /api/logs`
- `GET /api/logs/data`
- `POST /api/logs/logout`

Frontend:

- `/admin/status`
- `/g/[guildId]/admin/logs`

Bot:

- `/로그`
- `/되돌리기`

### `GET /api/guilds/:guildId/reverts/recent`

목적:

- 최근 되돌리기 가능 checkpoint 조회.
- 화면별 revert 후보를 보여준다.

Frontend:

- `/g/[guildId]/admin/logs`
- 일반 내전 pick-ban/result 화면
- 경매 draft/bracket 화면

### `POST /api/guilds/:guildId/reverts/:checkpointId/apply`

목적:

- 특정 checkpoint로 되돌린다.
- 범용 undo/redo가 아니라 정해진 운영 단계 복구만 처리한다.

권한:

- operator/admin.

Audit:

- 기록 필요.

### `POST /api/guilds/:guildId/reverts/apply-last`

목적:

- 현재 화면 또는 target 기준 마지막 revert checkpoint로 되돌리기.

사용 예:

```txt
series:{id} 마지막 game result 되돌리기
auction-tournament:{id} 마지막 bid 되돌리기
auction-match:{id} lineup lock 되돌리기
```

권한:

- operator/admin.

## Internal API

### `POST /internal/notify`

목적:

- API가 Bot에 generic notify를 보낸다.

### `POST /internal/heartbeat`

목적:

- Bot heartbeat 수신.

### Bot notify endpoints

Routes:

- `POST /internal/recruit-refresh`
- `POST /internal/series-created`
- `POST /internal/series-completed`
- `POST /internal/auction-tournament-completed`
- `POST /internal/auction-match-created`

목적:

- API 상태 전이 후 Bot이 Discord 메시지/알림을 best-effort로 발행하게 한다.

권한:

- internal token 필수.

## WebSocket

### `GET /ws`

목적:

- Frontend가 topic을 구독하고, Backend mutation 후 invalidate event를 수신한다.

Topics:

- `dashboard`
- `auction-dashboard`
- `recruitment:{id}`
- `series:{id}`
- `auction-recruitment:{id}`
- `auction-tournament:{id}`
- `auction-match:{id}`
- `leaderboard:{role}`
- `leaderboard:COMPOSITE`
- `user:{discordId}`

원칙:

- WebSocket payload는 원장 데이터 전체가 아니라 invalidate 신호다.
- Frontend는 invalidate 수신 후 해당 route/API 데이터를 재조회한다.

## Backend 우선순위

### P0

- OAuth/session.
- `/api/me`, `/api/me/perms`.
- recruitment read/detail/entry draft/reopen.
- series create/detail/pickban/games/result.
- `/api/champions`.
- `/dd` Data Dragon asset proxy/cache path.
- `/ws`.
- healthz.

Data Dragon cache 기준:

```txt
/api/champions
  -> champion catalog JSON
  -> memory cache
  -> 필요 시 Valkey shared cache

/dd/cdn/{version}/...
  -> champion image / spell image / item image
  -> nginx proxy cache + browser long cache
  -> Valkey에 image binary 저장 금지
```

### P1

- auction recruitment/tournament/match.
- users/profile/search.
- leaderboard.
- riot accounts.
- screening.

### P2

- logs/admin status.
- cleanup/stale/admin destructive endpoints.
- full internal notify coverage.
