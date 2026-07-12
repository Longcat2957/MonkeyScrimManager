# Backend Routing

작성일: 2026-07-12

## 범위

이 문서는 Rust/Axum API의 HTTP, OAuth/session, WebSocket, internal route를
정의한다. 상세 module, PostgreSQL transaction, test, phase는
[Backend Development Plan](../09-backend-development-plan.md)을 따른다.

Backend 책임:

- Discord OAuth/session
- guild membership/operator/service-admin 권한
- PostgreSQL read/write와 domain transition
- revision/idempotency/transaction
- audit/checkpoint/outbox
- Riot/Discord/Bot integration
- 제한적 WebSocket invalidate

비책임:

- SvelteKit layout/rendering
- Discord command parsing/message rendering
- localized relative time
- client-side MMR/낙찰/score 계산

## 공통 계약

- 모든 guild resource는 /api/guilds/{guildId}/ 아래에 둔다.
- ID와 Discord snowflake는 JSON string이다.
- JSON은 camelCase, timestamp는 UTC RFC 3339다.
- lane은 TOP/JUNGLE/MID/BOTTOM/SUPPORT다.
- workflow root DTO는 opaque `id`와 사람용 `displayNumber`를 분리한다. Route/deep link는 id,
  화면의 `#214`/`A-118` label은 displayNumber를 사용한다.
- revisioned editing aggregate mutation은 `expectedRevision`을 사용한다. Logout, Riot refresh,
  self preference PUT처럼 revision owner가 없는 command에 이를 억지로 추가하지 않는다.
- 위험 command는 Idempotency-Key를 요구한다.
- 다른 guild resource는 존재 여부를 숨기기 위해 404를 반환한다.

success:

```json
{
	"data": {},
	"meta": {
		"requestId": "01J..."
	}
}
```

conflict:

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

## Auth와 app context

### GET /oauth/discord/start

- Discord authorize URL로 redirect.
- state, PKCE, next allowlist.
- nginx가 설정한 canonical `X-Real-IP` 기준 process-local GCRA로 분당 30회까지 허용한다.
- 만료된 OAuth attempt는 새 attempt 저장과 같은 SQL statement에서 정리한다.

### GET /oauth/callback

- state/PKCE 검증.
- Discord token exchange와 user upsert.
- opaque server-side session cookie 발급.
- allowlist된 next로 redirect.
- state는 브라우저의 HttpOnly callback-path cookie와 함께 검증하고 DB에서 한 번만 소비한다.
- callback은 Discord 교환보다 먼저 attempt를 소비한다. 교환 결과가 불명확한 code를 재사용하지
  않으며, provider/DB 오류가 나면 새 로그인 흐름을 시작한다.
- nginx는 authorization code가 query log에 남지 않도록 callback access log를 끈다.

### POST /api/logout

- PostgreSQL session revoke.
- cookie 삭제.
- unsafe cookie-auth 요청은 exact Origin, CSRF cookie/header, DB hash를 모두 검증한다.

### GET /api/me

현재 사용자:

```text
discordId
displayName
avatarUrl
defaultGuildId
serviceAdmin
```

### GET /api/me/guilds

접근 가능한 guild와 각 guild의 현재 사용자 role을 반환한다.

### PUT /api/me/default-guild

request:

```json
{
	"guildId": "123456789012345678"
}
```

user_guild_preferences가 source of truth다. browser cookie는 root redirect
cache로만 사용한다.

### GET /api/guilds/{guildId}

guild layout context:

```text
id
displayName
iconUrl
memberCount
currentUserRole
isExternallyPublic
currentSeason
```

### POST /internal/guilds/{guildId}/setup

`/초기설정` 전용 Bot service-principal command다. Discord interaction ID를
`Idempotency-Key`로 전달한다. actor의 Discord guild 관리 권한은 Bot이 먼저 확인하고,
backend는 guild/user/member/config/ACTIVE season/counter를 한 transaction에서 upsert한다.

```json
{
	"actor": {
		"id": "100000000000000001",
		"displayName": "Longcat",
		"avatarHash": null,
		"roleIds": ["300000000000000001"]
	},
	"guildName": "MonkeyBot Test Guild",
	"guildIconHash": null,
	"operatorRoleId": "300000000000000001",
	"seasonName": "시즌 1"
}
```

같은 key와 body의 재전송은 같은 guild/season response를 반환한다. 같은 key의 다른 body는
`409 idempotency_conflict`다.

## Operations queue

### GET /api/guilds/{guildId}/operations

/g/{guildId}가 사용하는 처리 대기 projection.

row:

```text
id
kind       ENTRY | PICK_BAN | RESULT | AUCTION
title
status
detail
updatedAt
href
priority   urgent | normal
```

MVP에서 이 route는 WebSocket을 사용하지 않는다.
`kind=RESULT`인 actionable row의 href는 game 입력이 있는
`/g/{guildId}/series/{seriesId}/pick-ban`이다. 완료된 원장 링크만 `/result`를 사용한다.

## 일반 내전

### GET /api/guilds/{guildId}/recruitments

query:

```text
state=actionable | active | completed
cursor
limit
```

Bot/service principal은 필요한 create/member operation을 같은 application
service를 통해 사용한다.

### POST /api/guilds/{guildId}/recruitments

일반 모집 생성. Bot command가 주 사용자다.

Bot service principal과 `Idempotency-Key`가 필요하며 body의 actor role snapshot에 현재
`guild_config.operator_role_id`가 포함돼야 한다. backend는 actor user/member/role snapshot을
동기화하고 ACTIVE season과 guild counter를 잠근 뒤 모집과 빈 entry draft를 함께 만든다.

```json
{
	"actor": {
		"id": "100000000000000001",
		"displayName": "Longcat",
		"avatarHash": null,
		"roleIds": ["300000000000000001"]
	}
}
```

MVP의 `targetCount`는 10으로 고정한다. 재사용 entry component의 가변 prop만으로 1v1~4v4를
backend product contract에 추가하지 않는다.

### PUT /api/guilds/{guildId}/recruitments/{id}/message

모집 생성 후 Discord channel/message snowflake를 바인딩한다. Bot service principal,
`Idempotency-Key`, operator actor가 필요하다. Discord message 전송은 DB transaction 밖에서 먼저
완료하고 이 command를 호출한다.

### GET /api/guilds/{guildId}/recruitments/{id}

entry route snapshot:

```text
recruitment
  id, displayNumber, title, status, targetCount, revision
participants[]
entryDraft
  revision, assignments, updatedAt, updatedBy
synergies[]
headToHead[]
```

### PATCH /api/guilds/{guildId}/recruitments/{id}/entry-draft

최소 operation:

```json
{
	"expectedRevision": 42,
	"operation": {
		"type": "MOVE_PARTICIPANT",
		"participantId": "328516947210438656",
		"slot": "TEAM_1_TOP"
	}
}
```

slot null은 후보로 이동한다. frontend entry swap 규칙을 server에서 동일하게
적용한다. 전체 assignment JSON replace endpoint는 만들지 않는다.

### POST /api/guilds/{guildId}/recruitments/{id}/entry-draft/reset

entry draft를 모집 참가자 기준으로 명시적으로 초기화한다.

### POST /api/guilds/{guildId}/recruitments/{id}/reopen

CLOSED/CANCELLED 모집을 정책상 허용되는 OPEN 상태로 복구한다. operator,
expectedRevision, audit가 필요하다.

### POST /api/guilds/{guildId}/recruitments/{id}/members

### DELETE /api/guilds/{guildId}/recruitments/{id}/members/{userId}

Bot service principal과 `Idempotency-Key`를 사용한다. actor와 대상 user가 같으면 self join/leave,
다르면 operator 강제 참가/제거로 처리한다. OPEN recruitment row를 잠근 뒤 정원과 현재 참가 상태를
다시 검증한다. Bot은 PostgreSQL participant를 직접 수정하지 않는다.

### PUT /api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes

이미 JOINED인 참가자의 ordered preferred lanes를 교체한다. 1~5개의 중복 없는 canonical lane만
허용한다. self update가 아니면 operator가 필요하다.

### POST /api/guilds/{guildId}/recruitments/{id}/close

`recruit:start-entry:{id}` component command다. operator, OPEN, 참가자 10명을 transaction 안에서 다시
검증하고 CLOSED로 전이한 뒤 entry deep link가 포함된 최신 recruitment snapshot을 반환한다.

### POST /api/guilds/{guildId}/series

recruitment를 별도 series ID로 전환한다.

request:

```text
sourceRecruitmentId
expectedRevision
```

series.id와 recruitment.id가 같아야 한다는 규칙은 없다. 같은 recruitment에는 non-cancelled
series가 최대 하나다. 생성 되돌리기로 CANCELLED된 뒤 다시 전환하면 새 series ID를 발급한다.
현재 frontend에 format control이 없으므로 신규 series의 `bestOf`는 서버가 3으로 기록한다. DB와
read DTO는 legacy/향후 1/3/5를 보존한다.

### GET /api/guilds/{guildId}/series

query:

```text
state=active | completed
cursor
limit
```

active row는 다음 작업 URL을, completed row는 score/outcome/nullable winner/playedAt을 포함한다.

### GET /api/guilds/{guildId}/series/{id}

pick-ban input과 result ledger가 공통으로 읽는 series snapshot. 필요하면
view=input 또는 view=ledger query로 payload 크기를 나눈다.

input:

```text
series metadata(id/displayNumber/title)/revision/bestOf
teams와 lane별 participant
games와 current draft
sidePreset
picks/bans
updatedAt/updatedBy
```

ledger:

```text
series metadata/completedAt/score/outcome/terminalReason/nullable winner
teams/participants
game별 side/winner/durationSeconds(null 허용)/picks/bans
participant별 MMR before/after/delta
```

Custom match submit에는 duration 입력이 없으므로 값을 추측하거나 문자열로 합성하지 않는다.

### PATCH /api/guilds/{guildId}/series/{id}/pickban

operation:

```text
SET_SIDE
SET_PICK
CLEAR_PICK
SET_BAN
CLEAR_BAN
```

특정 game/player/side/index만 수정한다. Hard Fearless를 MVP server invariant로
강제하지 않는다.

### POST /api/guilds/{guildId}/series/{id}/games

단일 game 결과 확정.

```text
setNumber
expectedRevision
sidePreset
winner
picks
bans
```

winner는 필수, picks/bans는 optional이다. game/MMR/score/completion/checkpoint/
audit/outbox가 한 transaction이다.

### DELETE /api/guilds/{guildId}/series/{id}/games/last

Idempotency-Key와 expectedRevision이 필요하다. game void, MMR reversal,
score/status 복구를 한 transaction에서 수행한다.

### POST /api/guilds/{guildId}/series/{id}/complete

Bot operator 조기 종료 command다. `outcome=TEAM_1|TEAM_2|DRAW|CANCEL`, expectedRevision, reason을
받는다. DRAW는 현재 game score가 동률일 때만 `COMPLETED/EARLY_DRAW`로, TEAM 결과는
`COMPLETED/EARLY_WIN`으로, CANCEL은 `CANCELLED`로 전이한다. 기존 game/MMR은 보존하고 audit와
revision을 같은 transaction에서 기록한다.

### POST /api/guilds/{guildId}/series/{id}/revert

정의된 series checkpoint만 복구한다. 범용 undo endpoint가 아니다.
Series 생성 revert는 game이 없을 때 series를 CANCELLED, source recruitment를 CLOSED로 되돌린다.

### GET /api/guilds/{guildId}/series/{id}/balance.svg

필요 시 현재 result 공유용 SVG를 생성한다. UI 필수 read와 분리한다.

## 경매 내전

### GET /api/guilds/{guildId}/auction-recruitments

query state=actionable|active|completed.

### POST /api/guilds/{guildId}/auction-recruitments

Bot이 경매 모집을 생성한다.

### GET /api/guilds/{guildId}/auction-recruitments/{id}

```text
id
displayNumber
title
format             TEN_PERSON | TWENTY_PERSON
participantCount   10 | 20
teamCount          2 | 4
participants[]
setupDraft
  revision
  teams[] = teamIndex, captainId, initialPoints
  updatedAt, updatedBy
```

### PATCH /api/guilds/{guildId}/auction-recruitments/{id}/setup-draft

팀장/시작 point를 최소 operation으로 저장한다.

```text
ADD_CAPTAIN(captainId)
REMOVE_CAPTAIN(captainId)
SET_INITIAL_POINTS(teamIndex, points)
```

모든 operation은 `expectedRevision`을 포함한다. 전체 team setup JSON을 반복 PUT하지 않는다.
Captain은 frontend의 선택 순서대로 team index에 들어간다. 중간 captain을 제거하면 뒤 captain을
한 칸씩 당기고 team index별 시작 point는 그대로 유지한다.

### POST/DELETE /api/guilds/{guildId}/auction-recruitments/{id}/members/**

Bot operator의 강제 참가/제거.
추가 request는 `userId`, `declaredLane`을 받는다. lane을 생략하면 저장된 guild main lane을
사용하고 둘 다 없으면 `422 lane_required`다. 참가/취소와 captain 자동 해제는 setup draft와
같은 transaction에서 처리한다.

### POST /api/guilds/{guildId}/auction-recruitments/{id}/cancel

취소와 audit.

### POST /api/guilds/{guildId}/auction-tournaments

저장된 setup draft를 검증하고 tournament로 확정한다.

```json
{
	"sourceRecruitmentId": "0190f480-0000-7000-8000-000000000001",
	"expectedRevision": 4
}
```

tournament는 별도 opaque ID를 발급하고 source_recruitment_id unique FK로
연결한다. 같은 recruitment에는 non-cancelled tournament가 최대 하나다. captain/point는
request가 아니라 해당 revision의 `auction_setup_drafts` snapshot에서 복사한다. 생성 되돌리기
후 다시 시작하면 새 tournament ID를 발급한다.

### GET /api/guilds/{guildId}/auction-tournaments

query state=active|completed.

### GET /api/guilds/{guildId}/auction-tournaments/{id}

view query로 setup, draft, bracket summary를 선택할 수 있다.

draft snapshot:

```text
id, displayNumber, title, format, status, revision
teams와 point/member
candidateQueue
currentTarget
unsold
recentRevertableAction
updatedAt/updatedBy
```

### POST /api/guilds/{guildId}/auction-tournaments/{id}/commands

frontend command union:

```text
DRAW_NEXT
MARK_UNSOLD
AWARD_BID
ASSIGN_CURRENT
ASSIGN_UNSOLD
RESTORE_UNSOLD
REVERT_LAST
```

일곱 command 모두 `Idempotency-Key`와 expectedRevision을 포함한다. AWARD_BID는 낙찰, point 차감,
team 배치, checkpoint, audit, outbox를 한 transaction에서 처리한다.
`RESTORE_UNSOLD`는 active target이 없을 때 선택한 유찰 선수를 즉시 current target으로
복원하며 queue 뒤로 보내지 않는다.
`REVERT_LAST`로 선수가 current target으로 돌아오면 이전 lot을 재사용하지 않고 새 lot ID를
발급한다.

### POST /api/guilds/{guildId}/auction-tournaments/{id}/cancel

Bot operator의 조기 종료용 command. `Idempotency-Key`, expectedRevision, reason, audit가 필요하다.

### POST /api/guilds/{guildId}/auction-tournaments/{id}/bracket/commands

```text
ASSIGN_TEAM
CLEAR_SLOT
LOCK_BRACKET
```

20인 slot은 A_0, A_1, B_0, B_1이다. 10인은 2팀 결승을 만든다.
현재 frontend에 format 선택이 없으므로 `LOCK_BRACKET`이 만드는 신규 match는 BO3다.
Bracket 생성 revert는 game이 없을 때 기존 match를 CANCELLED로 보존하고 tournament를
BRACKET_SETUP으로 되돌린다.

### GET /api/guilds/{guildId}/auction-matches/{matchId}

```text
id, label, round
bestOf
leftTeamId/rightTeamId
leftScore/rightScore/winnerTeamId
lineupsLocked/resultEntered
revision
lineups
games
```

round는 SEMIFINAL 또는 FINAL, bestOf는 1/3/5다.

### PATCH /api/guilds/{guildId}/auction-matches/{matchId}/lineup

한 선수를 한 lane에 배정/해제한다. 전체 lineup replace를 반복하지 않는다.

### POST /api/guilds/{guildId}/auction-matches/{matchId}/lineup/lock

양 팀 TOP/JUNGLE/MID/BOTTOM/SUPPORT가 정확히 한 명씩이고 team membership과
중복이 유효해야 한다.

### POST /api/guilds/{guildId}/auction-matches/{matchId}/games

matchId, setNumber, match의 expectedRevision, game을 받는다. 별도 tournament revision은 받지 않는다.
서버는 tournament를 먼저 lock해 현재 status와 bracket 관계를 검증한 뒤 match revision을 비교한다.
lock 전 입력은 거부한다.
경매 game도 일반 game과 같은 guild MMR service/ledger를 사용한다.

### DELETE /api/guilds/{guildId}/auction-matches/{matchId}/games/last

game/MMR/match score/tournament state를 transaction으로 복구한다.

### GET /api/guilds/{guildId}/auction-tournaments/{id}/result

결과 overview:

```text
champion team
matches와 score
team captain/initial/current point
team member acquisition/bid point
participant game result와 MMR delta
revertable checkpoint
```

일반 참가자에게는 원장 조회이고 operator에게만 `결과 입력 되돌리기` 한 개를 노출한다. 이 action은
별도 tournament 전용 복구 API를 만들지 않고
`POST /api/guilds/{guildId}/reverts/{checkpointId}/apply`를 사용한다.

## 길드 전적과 랭킹

### GET /api/guilds/{guildId}/records/search

query:

```text
q
seasonId
type
lane
cursor
limit
```

summary:

- Discord/Riot display
- season lane MMR
- champion wins/losses/main lane
- 일반/10인 경매 series
- 20인 auction tournament grouping

expand detail:

- series stage/opponent/score
- set winner/lane/champion/MMR before/after
- 양 팀 lineup/pick/ban/MMR delta
- tournament roster/point/rank

내부 games가 source of truth다.

### GET /api/guilds/{guildId}/leaderboard

현재 frontend가 role별 정렬할 수 있는 player snapshot:

```text
id, name, tag, profileIconId
compositeMmr, mainRole, isMe
stats[lane] = mmr, games, wins, championId
```

MVP에서 records와 leaderboard는 WS를 사용하지 않는다.

## Riot 외부 전적

### GET /api/riot/records

query:

```text
query=GameName%23TagLine
region=kr
```

profile, rank, champion aggregate, recent match summary를 반환한다.

### POST /api/riot/records/refresh

사용자 요청 refresh. rate limit과 stale cache를 구분한다.

external match expand는 Riot 원본 데이터의 KDA/CS/gold/damage/vision/item을
표시할 수 있지만 guild custom match schema와는 별개다.

현재 게임/Spectator endpoint는 만들지 않는다.

## 사용자와 내 정보

### GET /api/users/{discordId}/profile

MVP는 현재 UI가 사용하는 displayName, Discord ID, 활동 guild 수, season
수부터 제공한다. MMR history, auction card, screening은 UI가 생기기 전
선행 구현하지 않는다.

### GET /api/me/champion-preferences

### PUT /api/me/champion-preferences/{lane}

lane별 ordered champion ID, 최대 10개.

### GET /api/me/riot-accounts

### POST /api/me/riot-accounts

### DELETE /api/me/riot-accounts/{puuid}

### PUT /api/me/riot-accounts/{puuid}/main

### POST /api/me/riot-accounts/{puuid}/refresh

account가 남아 있으면 main account가 정확히 하나여야 한다.

### GET /api/me/privacy

### PUT /api/me/privacy

```text
publicProfile
showRiotIds
allowExternalRecordSearch
```

## Guild admin

모든 route는 OPERATOR 권한을 검증하고 admin mutation은 해당 snapshot의
`expectedRevision`을 검증한다.

### GET /api/guilds/{guildId}/members

```text
revision       guild_admin_state[MEMBERS].revision
updatedAt
updatedBy
members[]       mainLane은 미등록/미설정 member에서 null 가능
```

### PATCH /api/guilds/{guildId}/members/{memberId}/mmr

```text
lane
delta
note
expectedRevision
```

summary MMR 직접 덮어쓰기가 아니라 MANUAL_ADJUSTMENT ledger를 남긴다.
`Idempotency-Key`가 필수이며 같은 key 재전송은 ledger를 중복 append하지 않는다.

### PATCH /api/guilds/{guildId}/members/{memberId}/status

ACTIVE 또는 INACTIVE.

### GET /api/guilds/{guildId}/settings

```text
displayName
operatorRoleId
recruitmentChannelId
logChannelId
seasonId
isExternallyPublic
revision
updatedAt
updatedBy
```

seasonId는 read-only다.

### PUT /api/guilds/{guildId}/settings

seasonId를 mutation payload에 허용하지 않는다.
현재 frontend component의 local snapshot type에는 read-only `seasonId`가 포함되어 있으므로 API
adapter는 editable 다섯 field만 명시적으로 골라 request DTO를 만든다. component object를 그대로
serialize하지 않는다.

### GET /api/guilds/{guildId}/reverts/recent

```text
revision       guild_admin_state[OPERATIONS].revision
updatedAt
updatedBy
checkpoints[]
```

이 revision은 APPLY/expire command concurrency token이며 checkpoint 목록의 ETag가 아니다. 새
checkpoint INSERT는 guild 전역 operation을 직렬화하지 않고 다음 GET에서 조회된다.

### POST /api/guilds/{guildId}/reverts/{checkpointId}/apply

checkpointId, reason, expectedRevision. 종속 결과가 생긴 안전하지 않은 revert는
거부한다.

### GET /api/guilds/{guildId}/audit-logs

cursor, category, actor, target, result filter.

웹과 Bot이 같은 관리 작업을 제공하면 같은 application command와 audit
transaction을 사용한다. 일괄 등록, 시즌 변경/초기화, 오래된 내전 정리는
MVP 웹에 노출하지 않는다.

### POST /api/guilds/{guildId}/seasons

Bot이 주로 호출하는 season rotate command다.

```text
name
expectedCurrentSeasonId
```

현재 ACTIVE season을 CLOSED로 보존하고 새 ACTIVE season을 만든다. `Idempotency-Key`가 필수이며
과거 game/MMR/auction row를 삭제하지 않는다. Web admin UI에는 노출하지 않는다.

```text
idempotency 확보
-> current ACTIVE season SELECT FOR UPDATE
-> expectedCurrentSeasonId 확인
-> 기존 season CLOSED + ended_at
-> 새 ACTIVE season insert
-> audit/idempotency response
-> commit
```

`seasons`의 guild별 ACTIVE partial unique가 concurrent rotate의 두 번째 commit을 차단한다.

## Service admin

### GET /api/admin/status

SERVICE_ADMIN만 접근한다.

```text
API
PostgreSQL
Bot heartbeat
Valkey
```

각 항목은 normal/delayed/down, latency/value, lastHealthyAt, issue를 가진다.
Data Dragon과 internal notify를 별도 tile로 만들지 않는다.

### GET /healthz

process liveness only.

### GET /readyz

PostgreSQL과 migration version 필수. Valkey 장애는 degraded로 보고한다.

### GET /metrics

VPS internal network only.

## Internal Bot API

internal route는 user cookie가 아니라 service principal을 사용한다.

### POST /internal/heartbeat

Bot status 갱신.

### POST /internal/notifications/claim

Bot worker가 available/expired-lease row를 claim한다. response는 notification ID, leaseToken,
channelId, type, payload, stable Discord nonce를 포함한다.

### POST /internal/notifications/ack

notification ID, leaseToken, Discord message ID를 받아 owner+token이 현재 lease와 일치할 때만
delivered로 전이한다.

### POST /internal/notifications/nack

notification ID, leaseToken, Discord status/error/retryAfter를 받는다. Backend가 retryable 여부,
backoff, MAX_ATTEMPTS, DELIVERY_UNKNOWN terminal 전이를 결정한다.

Bot의 domain command는 별도 약식 로직을 만들지 않고 위 guild-scoped
application command를 service principal로 호출한다. Discord interaction ID를
Idempotency-Key로 사용한다.

## WebSocket

### GET /ws

cookie session으로 handshake한다.

허용 topic:

```text
guild:{guildId}:recruitment:{id}
guild:{guildId}:series:{id}
guild:{guildId}:auction-recruitment:{id}
guild:{guildId}:auction-tournament:{id}
guild:{guildId}:auction-match:{id}
```

event:

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

원칙:

- subscribe마다 guild/resource 권한 확인
- full state/mutation payload 전송 금지
- reconnect 후 HTTP snapshot 재조회
- bounded queue와 slow-client 종료
- at-least-once event 중복 허용
- PostgreSQL outbox -> Valkey -> WS

## 구현 순서

P0:

- auth/session/guild context
- 최소 Bot service principal, 일반 모집 interaction, series created/completed notification
- operations와 series/auction list read
- recruitment entry
- series game/MMR/revert

P1:

- limited WS
- auction 10/20 end-to-end
- records/leaderboard/me/Riot

P2:

- guild admin
- remaining auction/admin Bot boundary와 heartbeat
- migration/hardening
