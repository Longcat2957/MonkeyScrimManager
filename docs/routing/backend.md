# Backend Routing

작성일: 2026-07-15

## 범위

이 문서는 Rust/Axum API의 HTTP, OAuth/session, WebSocket, internal route를
정의한다. 상세 module, PostgreSQL transaction, test, phase는
[Backend Development Plan](../09-backend-development-plan.md)을 따른다.

현재 endpoint와 DTO는 `apps/api/src/http/mod.rs`, handler/service, contract code와
[Current Operating Contract](../12-current-operating-contract.md)를 우선한다. 이 문서의 계획성 서술은
runtime과 충돌할 때 현재 구현을 덮어쓰지 않는다.

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
- `UserId`는 provider와 무관한 opaque JSON string이다. Discord 사용자는 snowflake, 개발용 더미
  사용자는 `dmy_<UUIDv7>`를 사용한다. Guild/role/channel/message/interaction ID는 계속 Discord
  snowflake다.
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
backend는 `INSTALLED|SETUP_REQUIRED`이고 `guild_config`가 없는 길드에만
guild/user/member/config/ACTIVE season/counter를 한 transaction에서 생성한다.
`recruitmentChannelId`는 필수 Discord channel snowflake이며, `entryChannelId`, `resultChannelId`,
`logChannelId`는 nullable snowflake다. setup 시점에는 Discord catalog가 아직 없을 수 있으므로 catalog 존재
검증은 하지 않는다. Bot이 같은 Discord guild의 게시 가능한 text/announcement channel인지 먼저 검증한 값만
전달한다.

```json
{
	"actor": {
		"id": "100000000000000001",
		"globalDisplayName": "Longcat",
		"displayName": "MSM Longcat",
		"avatarHash": null,
		"roleIds": ["300000000000000001"]
	},
	"guildName": "MonkeyBot Test Guild",
	"guildIconHash": null,
	"operatorRoleId": "300000000000000001",
	"recruitmentChannelId": "400000000000000010",
	"entryChannelId": null,
	"resultChannelId": null,
	"logChannelId": null,
	"seasonName": "시즌 1"
}
```

같은 key와 body의 재전송은 같은 guild/season response를 반환한다. 같은 key의 다른 body는
`409 idempotency_conflict`다. 설정 완료 뒤 새 interaction으로 다시 실행하면
`409 guild_already_configured`를 반환하고 길드 이름, 운영자 role, member, season을 변경하지 않는다.
초기화 이후 설정 변경은 `PUT /api/guilds/{guildId}/settings`에서 revision 검증과 감사 로그를 거쳐
수행한다.

### POST /internal/guilds/{guildId}/registrations

Discord `/등록 riot_id` 전용 Bot service-principal command다. Discord interaction snowflake를
`Idempotency-Key`로 전달하며, DM이 아닌 현재 guild의 실행자 본인만 대상이 된다.

```json
{
	"actor": {
		"id": "100000000000000001",
		"globalDisplayName": "Longcat",
		"displayName": "긴고양이",
		"avatarHash": null,
		"roleIds": ["300000000000000001"]
	},
	"gameName": "Longcat",
	"tagLine": "KR1",
	"platform": "KR"
}
```

`actor.id`가 전역 사용자 identity다. `globalDisplayName`은 `users.display_name`, `displayName`은 현재
서버의 `guild_members.display_name`에만 반영하며 어느 표시명도 key로 사용하지 않는다. Riot account의
canonical key는 provider가 확인한 PUUID다.

Riot resolve는 DB transaction 전에 수행한다. 성공한 resolved account로 짧은 transaction을 열어
`users`, 현재 `(guild_id, user_id)`의 `guild_members`, PUUID 기반 `riot_accounts`와 Discord role
snapshot을 함께 확정한다. 어느 한쪽만 저장된 상태로 성공을 응답하지 않는다.

성공은 `200`이며 data는 다음 형태다.

```text
member
  id
  displayName
  status            ACTIVE
riotAccount
  puuid
  gameName
  tagLine
  platform
  isMain
registration        CREATED | EXISTING
accountLink         CREATED | EXISTING
```

`registration`은 현재 guild membership의 생성 여부이고 `accountLink`는 전역 PUUID 연결의 생성 여부다.
같은 사용자가 같은 guild에서 반복 실행하면 둘 다 `EXISTING`일 수 있다. 다른 guild에서 같은 PUUID를
등록하면 `registration=CREATED`, `accountLink=EXISTING`으로 전역 계정만 재사용한다.

같은 idempotency key와 payload 재전송은 기존 response를 replay하고, 같은 key의 다른 payload는 `409
idempotency_conflict`다. 기존 member가 INACTIVE면 사용자/소속/role/Riot 계정을 갱신하거나 자동
재활성화하지 않고 `403 member_inactive`를 반환한다. 그 밖의 주요 오류는 `400 invalid_riot_id`, `404`
Riot account not found, `409 guild_setup_required|guild_inactive|riot_account_already_linked`, `429` provider
rate limit, `502` provider timeout/failure, `503` Riot service unavailable이다.

### PUT /internal/guilds/{guildId}/discord-catalog

Discord Bot service-principal이 길드 설정에서 선택 가능한 role/channel snapshot을 전체 교체한다.
Discord Bot token을 Axum에 전달하지 않으며 `MONKEYBOT_BOT_SERVICE_TOKEN` bearer만 사용한다. 이
endpoint는 interaction command가 아니므로 `Idempotency-Key`를 사용하지 않는다.

```text
roles[] (최대 250)
  id          Discord role snowflake
  name        trim 후 1..100자
  position    0 이상 integer
  managed     boolean
channels[] (최대 500)
  id          Discord channel snowflake
  name        trim 후 1..100자
  parentName  trim 후 1..100자 | null
  position    0 이상 integer
  kind        TEXT | ANNOUNCEMENT
```

role/channel ID는 각 목록에서 중복될 수 없고 `@everyone` role(`role.id == guildId`)은 거부한다. 성공하면
두 catalog table을 한 transaction에서 교체하고 `{ syncedAt }`을 반환한다. 설정 전이거나 비활성인 guild는
404다. Bot은 ready와 60초 주기 full sync, guild/channel/role 변경 event의 1초 debounce, `/초기설정`
직후 동기화를 통해 이 endpoint를 호출한다. Channel projection은 Bot이 `ViewChannel`과 `SendMessages`
권한을 가진 text/announcement channel만 포함한다.

## Development 더미 사용자

더미 사용자는 test fixture가 아니라 development PostgreSQL에 지속되는 `identity_kind=DUMMY` 사용자다.
모든 경로는 `MONKEYBOT_DUMMY_USERS_ENABLED=true`일 때만 router에 등록한다. Production에서는 이 설정을
거절하고, 기능이 꺼진 환경에서는 아래 경로가 `403`이 아니라 `404`다.

공통 권한과 요청 규칙:

- 모든 read와 preview는 Discord OAuth session의 service admin만 호출한다.
- mutation은 session, exact Origin, CSRF cookie/header, visible ASCII 1..128 byte
  `Idempotency-Key`가 모두 필요하다.
- mutation replay 보존 기간은 24시간이다. 같은 key와 같은 요청은 저장 응답을 재생하고 다른 요청은
  `409 idempotency_conflict`, 처리 중이면 `409 idempotency_in_progress`다.
- 실제 로그인한 service admin이 audit actor다. 더미 사용자는 권한 principal이 될 수 없다.
- JSON body는 `deny_unknown_fields`이며 응답은 항상 authoritative post-commit snapshot이다.

### GET/POST /api/dev/dummy-users

GET query:

```text
query                 표시 이름 또는 stable key 부분 검색
state                 ACTIVE | ARCHIVED
guildId               해당 길드에 배치된 사용자
groupId               해당 재사용 그룹의 사용자
limit                 기본 50, 1..100
offset                기본 0, 0 이상
```

POST는 한 사용자를 생성한다. `stableKey`는 전역에서 유일한 1..64자 소문자 영숫자/`_`/`-` 값이고 첫
문자는 영숫자다. `displayName`은 trim된 1..100자다. `avatarSeed`, `notes`, privacy, 길드 배치,
합성 Riot 계정은 선택이다. Privacy 기본은 세 값 모두 `false`다.

```json
{
	"stableKey": "balanced-mid-01",
	"displayName": "MID 01",
	"privacy": {
		"publicProfileEnabled": false,
		"showRiotIds": false,
		"allowExternalRecordSearch": false
	},
	"guildMemberships": [
		{
			"guildId": "1525851181157580861",
			"displayName": "MID 01",
			"status": "ACTIVE",
			"mainLane": "MID",
			"laneMmr": {
				"top": 1500,
				"jungle": 1500,
				"mid": 1600,
				"bottom": 1500,
				"support": 1500
			}
		}
	]
}
```

사용자 생성과 포함된 privacy, 길드 소속, 활성 시즌 MMR, 합성 Riot 계정은 한 transaction이다.

### POST /api/dev/dummy-users/bulk

`{ users: CreateDummyUser[] }`로 1..100명을 한 transaction에서 생성한다. Batch 안의 stable key도
중복될 수 없다. 한 사용자라도 validation, guild, season, identity 제약에 실패하면 전체를 rollback한다.
성공은 `201 { total, users }`다.

### GET/PATCH/DELETE /api/dev/dummy-users/{userId}

GET은 privacy, 합성 Riot 계정, 다섯 lane 선호 챔피언, 길드 배치/MMR, group ID를 합친 projection을
반환한다. PATCH는 `expectedRevision`과 변경할 `displayName|avatarSeed|notes|privacy` 중 하나 이상을
받는다.

DELETE 전에 `GET /api/dev/dummy-users/{userId}/deletion-impact`로 다음 count를 확인한다.

```text
blockingReferences
  guildMemberships, recruitmentParticipants, auctionParticipants, gameParticipants,
  userSessions, discordIdentities, realRiotAccounts
removableReferences
  groupMemberships, championPreferences, syntheticRiotAccounts
```

blocking count가 하나라도 있으면 `canDelete=false`이며 DELETE는 `409 dummy_user_deletion_blocked`다.
연결 이력이 없는 경우에만 removable child와 사용자를 삭제한다. 이력이 있으면 archive를 사용한다.

### POST /api/dev/dummy-users/{userId}/archive

### POST /api/dev/dummy-users/{userId}/restore

두 경로 모두 `{ expectedRevision }`을 받는다. Archive는 계정 삭제와 분리된 profile 상태 전이이므로
`users.deleted_at`을 설정하지 않고 역사 조회를 보존하면서 신규 수정, 그룹 추가, 길드 배치, 모집 참가를
막는다.

Archive 직전 일반/경매 모집을 모두 검사한다. `OPEN|CLOSED` 모집에 `JOINED` 참가가 하나라도 있으면
`409 dummy_user_archive_blocked_by_recruitment`이고 transaction 전체를 rollback한다. 오류 details의
`joinedRecruitments`, `joinedAuctionRecruitments` count를 확인해 참가를 먼저 취소해야 한다. 성공 시
현재 ACTIVE인 길드 소속만 INACTIVE로 전환하고, 영향받은 각 길드의 MEMBERS revision과
`MEMBER_STATUS_CHANGED` audit을 함께 기록한다. Admin members는 WebSocket topic을 사용하지 않는 기존
계약을 따르므로 별도 invalidate outbox는 없다.

Restore는 profile을 ACTIVE로 바꾸지만 길드 소속은 자동으로 ACTIVE로 되돌리지 않는다. Archive 전에
이미 INACTIVE였던 소속과 archive가 바꾼 소속을 추측해 일괄 복구하지 않고, 필요한 길드만 별도 membership
PUT으로 명시적으로 활성화한다.

### GET /api/dev/dummy-users/catalog

`LEFT`가 아닌 guild의 `guildId`, 표시 이름, status, nullable active season ID를 반환한다.

### PUT/DELETE /api/dev/dummy-users/{userId}/guilds/{guildId}

PUT은 길드 표시명, `ACTIVE|INACTIVE`, nullable main lane, 선택적 다섯 lane MMR을 upsert한다. MMR을
전달했는데 활성 season이 없으면 `409 active_season_required`다. DELETE는 보존할 workflow/MMR/audit
이력이 없을 때만 허용하며, 이력이 있으면 `409 dummy_membership_has_history`이므로 PUT으로 INACTIVE를
기록한다.

### PUT /api/dev/dummy-users/{userId}/champion-preferences/{lane}

`{ championIds: [] }`로 lane 하나를 순서대로 전체 교체한다. 0..10개의 중복 없는 Data Dragon ID를
허용하며 빈 배열은 해당 lane을 비운다.

### PUT/DELETE /api/dev/dummy-users/{userId}/riot-account

PUT은 `platform`, `routingRegion`, `gameName`, `tagLine`과 선택적 `profileIconId`, `mainPosition`을 받아
`origin=DUMMY` projection을 저장한다. Riot provider를 호출하지 않는다. DELETE도 합성 계정만 제거한다.
DB는 `DISCORD` user가 `origin=RIOT`, `DUMMY` user가 `origin=DUMMY` 계정만 소유하도록 composite FK로
강제한다. 합성 계정은 public Riot search, autocomplete, refresh와 실제 PUUID 소유권 흐름에서 제외한다.

### GET/POST /api/dev/dummy-groups

Group은 길드에 종속되지 않는 재사용 가능한 순서 있는 더미 사용자 집합이다. POST body는 name(1..100),
nullable description(최대 500), 선택적 `memberIds`(최대 100)다. Group 이름은 case-insensitive unique이고
활성 더미 사용자만 포함한다.

### PATCH/DELETE /api/dev/dummy-groups/{groupId}

PATCH는 `expectedRevision`과 선택적 name/description을 받는다. DELETE는 group과 membership만 삭제하고
사용자는 삭제하지 않는다.

### PUT /api/dev/dummy-groups/{groupId}/members

`{ expectedRevision, memberIds }`로 최대 100명의 중복 없는 활성 더미 사용자를 요청 순서대로 전체
교체한다.

### GET /api/dev/guilds/{guildId}/recruitments/{recruitmentId}/participants

Service admin이면서 해당 active guild의 active operator여야 한다. 모집의 status/count/revision과 해당
guild에 배치된 모든 더미 후보의 profile state, membership status, `AVAILABLE|JOINED|WITHDRAWN` 상태를
반환한다.

### POST /api/dev/guilds/{guildId}/recruitments/{recruitmentId}/participants/preview

```json
{
	"action": "ADD",
	"userIds": ["dmy_01900000-0000-7000-8000-000000000001"]
}
```

DB를 변경하지 않는 read 성격의 POST라 CSRF와 idempotency key를 요구하지 않는다. 선택 개수/중복,
편집 가능 상태(`OPEN|CLOSED`), 길드 배치, archive/inactive, 기존 참가 상태, 정원 문제를 `issues[]`로
모두 반환하고 문제가 없을 때만 `canApply=true`다. HTTP 성공과 실제 적용 가능 여부를 혼동하지 않는다.

### POST/DELETE /api/dev/guilds/{guildId}/recruitments/{recruitmentId}/participants

POST는 ADD, DELETE는 REMOVE다. Body는 1..10개의 중복 없는 `userIds`와 preview에서 받은
`expectedRevision`이다. Server는 모집 row를 잠근 뒤 권한, 편집 가능 상태, revision, 더미/길드 상태,
기존 참가, 정원을 다시 검증한다. Discord 모집 중인 `OPEN`뿐 아니라 series 전환 전 entry 편성 상태인
`CLOSED`에서도 허용하되 `CONVERTED|CANCELLED`에서는 차단한다.

성공 transaction은 participant/preferred lane, recruitment revision, entry draft revision, guild audit,
outbox invalidate를 함께 확정한다. ADD는 더미 회원의 main lane이 있으면 첫 선호 lane으로 복사한다.
REMOVE는 entry assignment와 preferred lanes를 지우고 participant를 `WITHDRAWN`으로 보존한다.

### 더미 경로 오류 계약

| HTTP | code                                        | 의미/안전한 조치                                                 |
| ---- | ------------------------------------------- | ---------------------------------------------------------------- |
| 400  | `invalid_idempotency_key`                   | Header 누락, 중복, 0 byte, 128 byte 초과 또는 visible ASCII 아님 |
| 401  | session 인증 오류                           | Discord OAuth session 재인증                                     |
| 403  | `service_admin_required`                    | 로그인 사용자가 service admin이 아님                             |
| 403  | `operator_required`                         | 모집 대상 guild의 active operator가 아님                         |
| 404  | `dummy_user_not_found`                      | 더미 사용자가 없거나 해당 guild 후보가 아님                      |
| 404  | `dummy_group_not_found`                     | Group이 없음                                                     |
| 404  | `guild_not_found`                           | 배치할 guild가 없음                                              |
| 404  | `dummy_membership_not_found`                | 제거할 길드 소속이 없음                                          |
| 404  | `dummy_riot_account_not_found`              | 제거할 합성 Riot account가 없음                                  |
| 404  | `recruitment_not_found`                     | `(guildId, recruitmentId)` 모집이 없음                           |
| 409  | `dummy_user_archived`                       | Restore 후 수정/배치/참가                                        |
| 409  | `dummy_membership_has_history`              | 소속 삭제 대신 INACTIVE                                          |
| 409  | `dummy_user_deletion_blocked`               | `details.impact` 확인 후 archive                                 |
| 409  | `dummy_user_archive_blocked_by_recruitment` | 일반/경매 JOINED 참가를 취소한 뒤 archive                        |
| 409  | `active_season_required`                    | 초기 MMR 없이 배치하거나 활성 season을 먼저 생성                 |
| 409  | `dummy_stable_key_conflict`                 | 다른 stable key 사용                                             |
| 409  | `dummy_group_name_conflict`                 | 다른 group 이름 사용                                             |
| 409  | `dummy_group_member_unavailable`            | 누락/보관 사용자를 group에서 제거                                |
| 409  | `revision_conflict`                         | `details.currentRevision` snapshot을 다시 읽고 재시도            |
| 409  | `idempotency_conflict`                      | 새 key로 사용자의 새 의도를 제출                                 |
| 409  | `idempotency_in_progress`                   | 같은 요청 완료를 기다린 뒤 동일 key로 재조회                     |
| 409  | `recruitment_not_editable`                  | `OPEN/CLOSED` 외 상태에서는 참가 변경 차단                       |
| 409  | `dummy_membership_inactive`                 | 길드 소속을 명시적으로 ACTIVE로 전환                             |
| 409  | `already_participant`                       | 이미 JOINED인 사용자를 ADD함                                     |
| 409  | `not_participant`                           | JOINED가 아닌 사용자를 REMOVE함                                  |
| 422  | `invalid_dummy_user_input`                  | `details.field`의 validation 실패                                |
| 422  | `invalid_expected_revision`                 | 음수 revision                                                    |
| 422  | `invalid_dummy_participant_selection`       | Apply는 중복 없는 1..10명만 허용                                 |
| 422  | `recruitment_capacity_exceeded`             | `details`의 joined/requested/target count 확인                   |
| 503  | `database_unavailable`                      | PostgreSQL readiness 복구 후 재시도                              |

모든 오류는 `error.code`, 안전한 `error.message`, `error.requestId`를 보존한다. SQL, 내부 URL, token,
provider 원문은 사용자에게 노출하지 않는다. Preview의 business issue code는 같은 apply 오류 code를 사용해
적용 전에 원인을 그대로 보여준다.

## Operations queue

### GET /api/guilds/{guildId}/operations

/g/{guildId}가 사용하는 처리 대기 projection.

row:

```text
id
kind       ENTRY | PICK_BAN | AUCTION
title
status
detail
updatedAt
href
priority   urgent | normal
```

MVP에서 이 route는 WebSocket을 사용하지 않는다. 일반 내전의 pick-ban과 game 결과 입력은 모두
`kind=PICK_BAN`이며 href는 `/g/{guildId}/series/{seriesId}/pick-ban`이다. 완료된 원장 링크만 `/result`를
사용한다.

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

actionable row의 `href`는 `OPEN`이면 `/g/{guildId}/recruitments/{id}`로, `CLOSED`이면
`/g/{guildId}/recruitments/{id}/entry`로 내려준다. 클라이언트는 이 링크를 그대로 사용한다.

### POST /api/guilds/{guildId}/recruitments

일반 모집 생성. Discord `/내전모집`과 `/g/{guildId}/series`의 내전 모집 버튼이 같은 command를 쓴다.

Bot request는 service principal, Discord interaction `Idempotency-Key`, actor snapshot을 사용한다.
Browser request는 Discord OAuth session, CSRF, browser `Idempotency-Key`를 사용하며 actor를 지정할 수 없다.
두 경로 모두 다음 규칙을 같은 create command에 전달한다.

```json
{
	"bestOf": 3,
	"hardFearless": true,
	"expectedStartAt": "2026-07-26T20:00:00+09:00"
}
```

`bestOf`는 `1 | 3 | 5`, `hardFearless`는 boolean이다. Web과 최신 Bot은 항상 명시적으로 보내며,
호환 목적의 누락값만 server가 `BO3`, `true`로 해석한다. 둘 다 ACTIVE guild member면 생성할 수 있다.
`bestOf`가 이 범위를 벗어나면 server는 `422 invalid_recruitment_best_of`와
`details { field: "bestOf", allowedValues: [1,3,5], receivedValue }`를 반환한다.
`expectedStartAt`은 시간대가 포함된 RFC 3339 timestamp를 받으며, 누락하거나 빈 값이면 server가 수신 시점 30분 후로 정한다. 값을 직접 넣을 때 형식 오류는
`422 invalid_expected_start_at`과 `details.field="expectedStartAt"`을 반환한다. 생성 제목에는 KST
24시간 표기(`YYYY-MM-DD HH:mm (KST)`)로 고정한다.
Bot은 생성 전에 actor user/member/role snapshot을 동기화하고, backend는 ACTIVE season과 guild counter를
잠근 뒤 규칙을 가진 OPEN 모집과 빈 entry draft를 함께 만든다.

Browser 생성에는 `guild_config.recruitment_channel_id`가 반드시 설정되어 있어야 한다. 같은 transaction에
그 채널을 대상으로 하는 `bot_notification_outbox`의 `RECRUITMENT_MESSAGE_CREATE` row도 적재한다.
payload는 아래처럼 renderer가 필요한 완전한 recruitment snapshot을 고정한다.

```json
{
	"schemaVersion": 1,
	"recruitmentId": "01900000-0000-7000-8000-000000000001",
	"snapshot": { "recruitment": {}, "participants": [], "links": {} }
}
```

`201 Created`는 DB 상태와 발행 작업이 함께 확정되었다는 뜻이며 Discord 게시 완료를 의미하지 않는다.
모집 채널이 없으면 transaction 전체를 취소하고 `409 recruitment_channel_not_configured`를 반환한다.
Discord slash command 생성은 기존 interaction reply를 직접 게시하고 아래의 message bind command를
사용한다. Browser 생성은 그 bind command를 호출하지 않고 Bot relay의 typed acknowledgement로만 message
reference를 연결한다.

```json
{
	"actor": {
		"id": "100000000000000001",
		"globalDisplayName": "Longcat",
		"displayName": "MSM Longcat",
		"avatarHash": null,
		"roleIds": ["300000000000000001"]
	},
	"bestOf": 3,
	"hardFearless": true,
	"expectedStartAt": "2026-07-26T20:00:00+09:00"
}
```

MVP의 `targetCount`는 10으로 고정한다. 재사용 entry component의 가변 prop만으로 1v1~4v4를
backend product contract에 추가하지 않는다.

### PUT /api/guilds/{guildId}/recruitments/{id}/message

모집 생성 후 Discord channel/message snowflake를 바인딩한다. Bot service principal과
`Idempotency-Key`가 필요하며, actor는 모집 생성자이거나 operator여야 한다. Discord message 전송은 DB
transaction 밖에서 먼저 완료하고 이 command를 호출한다. 이 route는 Discord interaction으로 시작된
`/내전모집` reply bind 전용이다. Browser 생성의 queue worker는 아래 internal
`/internal/notifications/recruitment-message/ack`를 사용한다.

### GET /api/guilds/{guildId}/recruitments/{id}

엔트리 모집 snapshot:

```text
recruitment
  id, displayNumber, title, status, targetCount, bestOf, hardFearless, revision
participants[]
links
  entry
```

ACTIVE guild member만 조회한다. `OPEN`은 Discord에서 참가/취소/선호 라인을 수집하는 엔트리 모집
단계이고, `CLOSED`일 때만 Web 엔트리 편성 화면으로 이동한다. `CANCELLED` 모집은 이 read route와
actionable list에서 숨긴다. 취소 mutation의 idempotency replay 응답만 마지막 CANCELLED snapshot을
반환한다.

### DELETE /api/guilds/{guildId}/recruitments/{id}

Browser session, CSRF, browser `Idempotency-Key`를 사용하는 bodyless soft-cancel command다. `OPEN` 또는
`CLOSED` 모집을 생성자 또는 현재 guild의 active operator가 취소할 수 있다. participant와 entry
draft/history row는 보존하고 recruitment status를 `CANCELLED`로 전이하며 audit 및 invalidate outbox를
같은 transaction으로 기록한다. `CLOSED` 취소는 기존 assignment를 물리 삭제하지 않으며, entry-draft
snapshot의 `canCancel`은 이 권한과 상태를 반영한다. 동일 idempotency key는 취소 snapshot을 replay하고,
새 key로 다시 취소하면 `409 invalid_recruitment_state`를 반환한다. 이미 message reference가 있으면 같은
transaction에 `RECRUITMENT_MESSAGE_DELETE` notification을 적재한다. 초기 `CREATE`가 아직 bind되기 전
취소되면 create ack transaction이 현재 CANCELLED 상태를 확인해 DELETE notification을 후속 적재하므로,
오래된 create payload가 취소된 모집을 남겨 두지 않는다.

### GET /api/guilds/{guildId}/recruitments/{id}/entry-draft

ACTIVE guild member의 session read다. 아래 `EntryDraftSnapshot`을 반환한다. `OPEN`과 `CLOSED`만
읽을 수 있으며, 취소된 모집은 `404`로 숨긴다.

```text
recruitment
  id, displayNumber, title, status OPEN|CLOSED, targetCount 10, bestOf 1|3|5,
  hardFearless boolean, revision, updatedAt
draft
  revision, updatedAt, updatedBy { id, displayName } | null
  participants[]
    id, displayName, preferredLanes, mainLane | null, mmr, games, wins,
    riotTier | null, profileIconId | null, championIds
  assignments[]
    participantId, team TEAM_1|TEAM_2, lane
headToHead[]
  lane, userAId, userBId, plays, userAWins, userBWins
capabilities
  canEdit, canReset, canCreateSeries, canCreateAuctionRecruitment,
  canSendParticipantConfirmationDms, canReopen, canCancel
```

`capabilities`는 현재 viewer와 상태에서 계산한 snapshot hint다. `canCancel`은 생성자 또는 active
operator가 `OPEN|CLOSED` 모집을 취소할 수 있을 때 true다. 취소는 recruitment lifecycle command이며
entry-draft route 자체는 취소 후 `404`가 된다. `canCreateSeries`는 current active operator가 `CLOSED`
모집의 정확히 10명 JOINED 참가자를 `TEAM_1|TEAM_2`의 `TOP|JUNGLE|MID|BOTTOM|SUPPORT` 10개 slot에 각각
한 번씩 배치했을 때만 true다. 이 값은 UI hint이므로 시리즈 시작 command는 transaction 안에서 같은
조건을 다시 검증한다. `canCreateAuctionRecruitment`는 현재 active operator의 `CLOSED` 일반 모집에
정확히 10명의 JOINED 참가자가 있고, 아직 전환되지 않았으며 active series가 없을 때 true다. 이 경로는
lane assignment를 요구하지 않는다. 경매 전환 command도 source 모집 revision과 상태, JOINED 정원, 기존
전환 불가 상태를 transaction 안에서 다시 검증한다.
`canSendParticipantConfirmationDms`는 current active operator의 `CLOSED` 모집에서만 true다.

`riotTier`는 참가자의 `origin=RIOT` 주 계정과 일치하는 캐시 전적 snapshot의
`RANKED_SOLO_5x5`만 compact 표기(`P3`, `E2` 등)로 투영한다. 이 read는 Riot API를 호출하거나 Riot ID,
PUUID를 노출하지 않는다. 주 계정, cache, 솔로랭크가 없거나 cache 값이 canonical tier/division이 아니면
`null`이다.

`headToHead`는 현재 모집의 JOINED 참가자끼리 같은 lane에서 맞붙은 현재 season의 확정 일반 내전
game만 집계한다. 취소된 game과 경매 game은 제외한다. 각 row의 `userAId`와 `userBId`는 stable ID 순으로
고정하며 두 win 수의 합은 `plays`와 같다.

### PATCH /api/guilds/{guildId}/recruitments/{id}/entry-draft

Browser session, CSRF, browser `Idempotency-Key`가 필요하다. 현재 active operator만 `CLOSED` 모집의
draft revision을 기준으로 수정할 수 있다.

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

`slot`은 `TEAM_1|TEAM_2`와 `TOP|JUNGLE|MID|BOTTOM|SUPPORT`를 조합한 문자열 또는 `null`이다. `null`은
후보로 이동한다. 대상 slot에 다른 참가자가 있으면 server는 이전 slot으로 교환하고, 이동한 참가자가
후보였다면 기존 occupant를 후보로 보낸다. 전체 assignment JSON replace endpoint는 만들지 않는다.
성공과 idempotency replay 모두 최신 `EntryDraftSnapshot`을 반환하며, draft revision mismatch는
`entry_draft_revision_conflict`로 반환한다. 제한적 realtime에서 이 mutation의 invalidate는
`guild:{guildId}:recruitment-entry-draft:{id}` topic과 `draft.revision`을 사용하며, recruitment topic에
draft revision을 넣지 않는다.

### POST /api/guilds/{guildId}/recruitments/{id}/entry-draft/reset

같은 Browser session, CSRF, browser `Idempotency-Key`와 `{ "expectedRevision": <draft revision> }`를
사용한다. 현재 active operator가 `CLOSED` 모집의 모든 assignment를 제거하고 최신
`EntryDraftSnapshot`을 반환한다. 제한적 realtime invalidate는 entry-draft topic과 새 draft revision을
사용한다.

### POST /api/guilds/{guildId}/recruitments/{id}/reopen

같은 Browser session, CSRF, browser `Idempotency-Key`와
`{ "expectedRevision": <recruitment revision> }`를 사용한다. 현재 active operator만 `CLOSED` 모집을
`OPEN`으로 되돌릴 수 있다. 기존 entry assignment는 보존하며, recruitment revision을 증가시키고 bound
Discord 모집 메시지가 있으면 같은 transaction에 UPDATE delivery를 적재한다. 성공과 idempotency replay는
최신 `EntryDraftSnapshot`을 반환한다. 제한적 realtime에서 recruitment lifecycle invalidate는 recruitment
topic과 recruitment revision을, draft metadata가 바뀐 경우 entry-draft invalidate는 entry-draft topic과
draft revision을 각각 사용한다. `CANCELLED` 모집 또는 series 생성은 이 endpoint가 처리하지 않는다.

### POST /api/guilds/{guildId}/recruitments/{id}/auction-recruitment

일반 모집을 10인 경매 모집으로 전환하는 Browser 전용 command다. Discord OAuth session, CSRF, browser
`Idempotency-Key`와 아래 body가 필요하며, 현재 active operator만 호출할 수 있다.

```json
{
	"expectedRevision": 12,
	"expectedStartAt": "2026-07-26T20:00:00+09:00"
}
```

`expectedRevision`은 entry draft revision이 아니라 source recruitment의 현재 `revision`이다.
`expectedStartAt`은 시간대가 포함된 RFC 3339 timestamp를 받으며, 누락하거나 빈 값이면 server가 수신 시점 30분 후로 정한다. 값을 직접 넣을 때 형식이 맞지 않으면
`422 invalid_expected_start_at`과 `details.field="expectedStartAt"`을 반환한다. Backend는 source가
`CLOSED`인지, 정확히 10명이 JOINED인지, source가 아직 전환되지 않았는지, active series가 없는지를
transaction 안에서 다시 확인한다. lane assignment는 이 전환의 조건이 아니다.

성공하면 source 모집의 format/rules를 복사한 CLOSED `TEN_PERSON` 경매 모집과 setup draft/team slot을
만들고 JOINED roster를 복사한다. 생성된 모집 제목의 예상 시작 표기는 항상 KST offset과 `(KST)`를 붙여
보여 주므로, Browser의 local datetime 입력이 UTC로 전송되어도 운영자와 Discord가 같은 한국 시각을 읽는다.
source recruitment는 `CONVERTED`가 되며 audit, idempotency, realtime, Discord 모집 발행 outbox도 같은
transaction에서 확정한다.

### POST /api/guilds/{guildId}/recruitments/{id}/members

Bot request는 service principal과 Discord actor/member snapshot을 사용한다. actor와 대상 user가 같으면
self join, 다르면 operator 강제 참가로 처리한다. Browser request는 session, CSRF, browser
`Idempotency-Key`, 아래 body를 사용하며 active operator만 호출할 수 있다.

```json
{ "memberId": "dmy_01900000-0000-7000-8000-000000000001" }
```

Browser의 `memberId`는 현재 guild의 ACTIVE member여야 하며 Discord user와 development dummy user를
같은 opaque `UserId` 계약으로 다룬다. 모집 row를 잠근 뒤 OPEN 상태, 정원, 중복 참가, target member
상태를 다시 검증한다. Bot과 browser 모두 PostgreSQL participant를 직접 수정하지 않고 동일 application
service를 사용한다. Browser 수동 추가가 이미 bind된 Discord 모집 메시지를 변경하면 같은 transaction에
`RECRUITMENT_MESSAGE_UPDATE`를 적재한다. initial create delivery와 경합한 경우 typed create ack이 최신
revision을 비교하고 필요한 UPDATE를 후속 적재한다.

### DELETE /api/guilds/{guildId}/recruitments/{id}/members/{userId}

Bot request는 service principal과 Discord actor를 사용한다. 자기 자신은 직접 참가 취소할 수 있고, 다른
참가자를 대상으로 하면 operator가 필요하다. Browser request는 session, CSRF, browser
`Idempotency-Key`, 빈 `{}` body를 사용하며 현재 active operator만 호출할 수 있다. 대상 participant를
`WITHDRAWN`으로 전환하고 이미 만든 entry assignment를 지운 뒤 recruitment/draft revision을 올린다.
이미 bind된 Discord 모집 메시지는 같은 transaction에 `RECRUITMENT_MESSAGE_UPDATE`를 적재한다.
개발용 더미 참가자 일괄 추가/제거도 bound 모집이면 같은 canonical UPDATE 대기열을 적재하므로, UI에서
실제 모집 흐름을 검증해도 Discord 모집 현황이 뒤처지지 않는다.

### PUT /api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes

이미 JOINED인 참가자의 ordered preferred lanes를 교체한다. 1~5개의 중복 없는 canonical lane만
허용한다. self update가 아니면 operator가 필요하다.

### POST /api/guilds/{guildId}/recruitments/{id}/close

`recruit:start-entry:{id}` Bot component command와 Web의 엔트리 편성 시작 action이 같은 command를 쓴다.
Bot request는 operator actor snapshot을, Browser request는 session, CSRF, browser `Idempotency-Key`, 빈 `{}`
body를 사용한다. 두 흐름 모두 current active operator, OPEN, 참가자 10명을 transaction 안에서 다시
검증하고 CLOSED로 전이한 뒤 entry deep link가 포함된 최신 recruitment snapshot을 반환한다. Browser action은
이미 bind된 Discord 모집 메시지의 `RECRUITMENT_MESSAGE_UPDATE`를 같은 transaction에 적재한다.
마감 자체는 참가 확정 DM을 자동 enqueue하지 않는다.

### POST /api/guilds/{guildId}/recruitments/{id}/participant-confirmation-dms

Browser session, CSRF, browser `Idempotency-Key`, 빈 `{}` body를 사용하는 수동 발송 요청이다. current active
operator만 `CLOSED` 모집에서 호출할 수 있다. canonical `JOINED` Discord 참가자별
`RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE` notification을 적재하며 개발 더미 사용자는 제외한다.
같은 CLOSED revision·recipient 조합은 deduplication key로 한 번만 적재된다. 응답은
`queuedRecipientCount`와 `alreadyQueuedRecipientCount`를 반환한다. Discord DM 차단·권한 오류는 notification
failure로 남기며 모집 상태를 되돌리지 않는다.

### POST /api/guilds/{guildId}/series

일반 내전의 엔트리 편성을 실제 series로 전환하는 browser command다. Discord OAuth session, CSRF,
browser `Idempotency-Key`와 아래 body가 필요하며, 현재 active operator만 호출할 수 있다.

```json
{
	"sourceRecruitmentId": "01900000-0000-7000-8000-000000000001",
	"expectedRevision": 12
}
```

`expectedRevision`은 entry draft revision이 아니라 source recruitment의 현재 `revision`이다. backend는
source recruitment를 먼저 잠근 뒤 entry draft를 잠그고, source가 `CLOSED`인지, 정확히 10명의 JOINED
참가자가 있는지, 각 참가자가 중복 없이 `TEAM_1|TEAM_2`의
`TOP|JUNGLE|MID|BOTTOM|SUPPORT` 10개 slot을 모두 채웠는지를 다시 확인한다.

성공 transaction은 source recruitment의 season에서 각 배정 lane의 MMR row를 잠근다. 아직 MMR row가
없으면 1500으로 초기화한 후 `series_participants`에 모두 `CAPTURED` snapshot으로 고정한다. 이어서
source recruitment의 `best_of`, `hard_fearless`를 복사한 별도 series ID의 `IN_PROGRESS` series를 만들고,
source recruitment를 `CONVERTED`로 전이하며, recruitment revision, audit, idempotency response, realtime
outbox를 함께 확정한다. 이미 bound Discord 모집 메시지가 있으면 같은 transaction에
`RECRUITMENT_MESSAGE_UPDATE`를 적재하여 `시리즈 진행 중` 상태로 바꾼다.

`guild_config.entry_channel_id`가 설정되어 있으면 같은 transaction에 `ENTRY_CONFIRMATION_CREATE`도
적재한다. payload는 immutable series rule, 확정된 `TEAM_1|TEAM_2` roster와 lane, 그리고 pick-ban deep
link를 고정한다. Bot relay는 이 immutable snapshot을 SVG 기반 PNG 엔트리 카드로 렌더링해 entry channel에
게시한 뒤 generic acknowledgement를 보내며, Discord delivery 실패는 retry/terminal failure로만 기록하고
series 생성 성공을 rollback하지 않는다.
entry channel이 `NULL`이면 이 notification만 생략하고 series 생성은 정상적으로 `201 Created`를 반환한다.

```json
{
	"data": {
		"series": {
			"id": "01900000-0000-7000-8000-000000000002",
			"href": "/g/1525851181157580861/series/01900000-0000-7000-8000-000000000002/pick-ban"
		}
	},
	"meta": { "requestId": "01900000-0000-7000-8000-000000000003" }
}
```

동일 `Idempotency-Key`와 body는 같은 `201` 결과를 replay한다. stale recruitment revision은
`409 recruitment_revision_conflict`, source가 더 이상 `CLOSED`가 아닌 경우는
`409 series_source_not_ready`, 10인 편성이 완성되지 않은 경우는
`422 entry_draft_incomplete`(joined/assigned/target count detail 포함)로 반환한다. 이 endpoint는 format
값을 받거나 수정하지 않는다. 모집 생성 시 확정한 규칙을 복사하므로 Bot과 Web 모집이 같은 series
규칙을 보장한다.

### GET /api/guilds/{guildId}/series

query:

```text
state=active | completed
cursor
limit
```

active row는 다음 작업 URL을, completed row는 score/outcome/nullable winner/playedAt을 포함한다.

### GET /api/guilds/{guildId}/series/{id}

현재 `IN_PROGRESS` 일반 series의 pick-ban workspace read다. Discord OAuth session의 active guild
member만 읽을 수 있다. 응답은 immutable `series_participants` roster(MMR snapshot, tier, 선호 챔피언),
현재 set의 draft, 이미 확정된 set 요약, lifecycle capability를 반환한다. `expectedRevision`에 쓰는 값은
`series.revision`이다.

```text
data.series: id/displayNumber/title/bestOf/hardFearless/status/revision/sourceRecruitmentId/updatedAt
data.teams: TEAM_1|TEAM_2 각각 TOP/JUNGLE/MID/BOTTOM/SUPPORT 5명
data.currentGame: setNumber/sidePreset/winner/picks(10명 모두)/bans(BLUE·RED 각 5칸)/patchVersion
data.games: 확정된 set 순서의 sidePreset/winner/picks/bans/patchVersion
data.capabilities: canEdit/canSubmit/canEarlyComplete/canRevertLastGame/canRevert/canDelete
```

`currentGame`은 다음 미확정 set이며 `games`는 결과 ledger의 축약본이다. `canEdit`, `canSubmit`,
`canRevertLastGame`, `canRevert`, `canDelete`는 모두 current active operator에게만 true다. Custom match
submit에는 duration 입력이 없으므로 값은 추측하거나 문자열로 합성하지 않는다.

### PATCH /api/guilds/{guildId}/series/{id}/pickban

operation:

```text
SET_SIDE
SET_PICK
CLEAR_PICK
SET_BAN
CLEAR_BAN
APPLY_BULK
```

Browser session, CSRF, current active operator 권한과 아래 body가 필요하다. Idempotency-Key는 필요하지
않으며 각 cell action은 `series.revision`을 정확히 하나 올리고 updated actor/time, audit, series realtime
outbox를 같은 transaction에 기록한다.

```json
{
	"expectedRevision": 12,
	"operation": { "type": "SET_PICK", "playerId": "100000000000000001", "championId": "Ahri" }
}
```

`SET_SIDE(sidePreset=DEFAULT|SWAPPED)`, `SET_PICK`, `CLEAR_PICK`,
`SET_BAN(side=BLUE|RED, slot=1..5)`, `CLEAR_BAN`을 허용한다. `hardFearless`는 이 endpoint에서
수정할 수 없다. 모집 생성 시 확정된 series 규칙 snapshot을 읽기와 validation에만 사용한다.

`APPLY_BULK`는 일괄 입력 전용이며 `items`에 최대 20개의 semantic `SET_PICK` 또는 `SET_BAN`만 담는다.
중첩 operation과 clear는 허용하지 않고, 동일 player pick 또는 동일 ban slot을 한 요청 안에서 두 번
지정할 수 없다. 모든 item은 최종 draft에 원자적으로 반영된 후 한 번만 validation/revision/outbox를
실행한다.

```json
{
	"expectedRevision": 12,
	"operation": {
		"type": "APPLY_BULK",
		"items": [
			{ "type": "SET_PICK", "playerId": "100000000000000001", "championId": "Ahri" },
			{ "type": "SET_BAN", "side": "BLUE", "slot": 1, "championId": "Yone" }
		]
	}
}
```

Pick user는 frozen 10인 roster에 있어야 하며 champion ID는 1..64 ASCII 영숫자다. 같은 set에서 pick과
ban을 합쳐 챔피언이 중복되면 `422 duplicate_series_game_champion`을 반환한다. `hardFearless=true`이면
이전 confirmed set에서 픽된 챔피언을 후속 set에서 다시 픽할 수 없고, `422 hard_fearless_champion_used`를
반환한다. 이전 ban은 Hard Fearless 대상이 아니다. 이 validation은 draft cell, bulk, game submit 모두에서
server-side로 다시 수행한다.

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

Browser session, CSRF, UUID browser `Idempotency-Key`, active operator 권한이 필요하다. `winner`는 필수,
`picks`/`bans`/`patchVersion`은 optional이다. 제공된 `picks`는 frozen 10명 전체 key를 포함해야 하고,
`bans`는 BLUE/RED 각각 정확히 5칸이어야 한다. current next set만 확정할 수 있다.

game, participant/bans, ELO MMR ledger, series score/completion, series revision, audit, idempotency response,
realtime outbox는 하나의 transaction이다. 성공은 `201`이며 다음을 반환한다.

```text
series: id/status(IN_PROGRESS|COMPLETED)/revision
confirmedGameCount
nextHref: active이면 /pick-ban, 완료면 /result
```

score가 series 형식의 필요 승수에 도달해 `COMPLETED/SCORE_REACHED`로 전이할 때만, configured
`guild_config.result_channel_id`에 `SERIES_RESULT_CREATE` Discord notification도 같은 transaction에 적재한다.
진행 중 set 확정에는 결과 카드를 만들지 않는다. Discord delivery는 outbox worker의 재시도 대상이며 game 확정
응답을 기다리거나 rollback하지 않는다. result channel이 `NULL`이면 시리즈 결과는 정상 확정하고 notification만
생략한다. `entry_channel_id`는 결과 notification fallback이 아니다.

새 결과 notification은 `schemaVersion: 2`다. immutable snapshot은 series metadata/score/terminal reason,
고정된 1팀·2팀 lane roster, confirmed set마다 `winner`, `sidePreset`, 두 팀의 lane-ordered pick과
`bans.blue`/`bans.red` 5칸, `patchVersion`, result deep link를 포함한다. MMR 및 MMR delta는 Discord 결과
카드 contract에 넣지 않는다. 이미 outbox에 남은 `schemaVersion: 1` compact payload는 재작성하거나 replay하지
않으며, Bot이 legacy renderer로 delivery한다. 새 backend emitter만 v2를 생성한다.

### DELETE /api/guilds/{guildId}/series/{id}/games/last

Browser session, CSRF, UUID browser `Idempotency-Key`, active operator 권한과 `{ "expectedRevision": n }`이
필요하다. `IN_PROGRESS|COMPLETED`의 마지막 confirmed game만 void하고 MMR reversal, score/status 복구,
series revision, audit/outbox를 한 transaction에서 수행한다. 성공은 `200`이며 POST game과 같은
`series/confirmedGameCount/nextHref` response를 반환한다. 이후 manual MMR 변경 또는 아직 되돌리지 않은
후속 game 결과가 있으면 `409 series_game_reversal_not_safe`로 거절한다.

### GET /api/guilds/{guildId}/series/{id}/result

완료된 일반 series의 immutable result ledger다. active guild member가 읽을 수 있고 `COMPLETED`가 아니면
`409 series_result_unavailable`을 반환한다.

```text
data.series: id/displayNumber/title/bestOf/status/revision/sourceRecruitmentId/
             winningTeam/terminalReason/startedAt/endedAt
data.teams: frozen TEAM_1|TEAM_2 roster
data.games: setNumber/sidePreset/winner/picks/bans/patchVersion/durationSeconds/mmrChanges
data.score: team1/team2
data.capabilities: canRevertLastGame/canDelete (operator only)
```

result read에는 WebSocket을 사용하지 않는다. result에서 마지막 game을 되돌리면 series가 `IN_PROGRESS`로
복귀하므로 이후 GET result는 `series_result_unavailable`을 반환하고 client는 `/pick-ban`으로 이동한다.

### POST /api/guilds/{guildId}/series/{id}/complete

현재 진행 중인 일반 내전을 브라우저에서 조기 종료하는 lifecycle command다. Browser session, CSRF,
UUID `Idempotency-Key`, active operator 권한과 아래 body가 필요하다.

```json
{ "expectedRevision": 4, "outcome": "TEAM_1" }
```

`outcome`은 `TEAM_1 | TEAM_2 | DRAW`만 허용한다. 아직 결착이 나지 않은 `IN_PROGRESS` BO3/BO5에서
확정 game이 하나 이상 있을 때만 실행할 수 있다. `DRAW`는 현재 확정 스코어가 동률일 때만
`COMPLETED/EARLY_DRAW`로 전이하고, 팀 결과는 `COMPLETED/EARLY_WIN`으로 전이한다. 현재 입력 중인
`DRAFT` 픽밴은 결과에 포함하거나 void하지 않으며, 이후 마지막 확정 game을 되돌려 `IN_PROGRESS`로
복귀할 때 다시 사용할 수 있도록 보존한다.

기존 confirmed game과 해당 MMR ledger는 그대로 유지한다. revision, audit
`SERIES_EARLY_COMPLETED`, series realtime outbox, idempotency response는 하나의 transaction에서 기록한다.
`result_channel_id`가 설정되어 있으면 같은 transaction에 `SERIES_RESULT_CREATE` 결과 카드 notification도
적재한다.
성공은 `200`이고 game confirm과 같은 `series/confirmedGameCount/nextHref`를 반환하며 `nextHref`는
`/result`다. BO1 또는 확정 game이 없으면 `409 series_early_completion_unavailable`, 동률이 아닌
`DRAW`이면 `409 early_draw_requires_tied_score`를 safe details와 함께 반환한다. 취소는 이 endpoint의
outcome이 아니며 기존 `POST .../revert` 또는 `DELETE` lifecycle을 사용한다.

### POST /api/guilds/{guildId}/series/{id}/revert

첫 lifecycle checkpoint를 복구한다. 범용 undo endpoint가 아니다. Browser session, CSRF,
`Idempotency-Key`와 아래 body가 필요하며 active operator만 호출할 수 있다.

```json
{ "expectedRevision": 0 }
```

source recruitment -> series 순서로 lock하고, series가 `IN_PROGRESS|COMPLETED`, source가 `CONVERTED`,
`revision`이 일치하는지 한 transaction에서 다시 검증한다. confirmed game이 있으면 마지막 set부터 역순으로
`VOIDED`/MMR reversal을 수행한 뒤 series를 retained history의 `CANCELLED`/`terminal_reason=CANCELLED`로
전이하고 source recruitment를 `CLOSED`로 복구한다. 어느 game의 reversal도 안전하지 않으면 전체 transaction을
rollback하고 `409 series_game_reversal_not_safe`를 반환한다. audit와 두 aggregate의 realtime outbox도 같은
transaction에 기록한다.

```json
{
	"data": { "nextHref": "/g/{guildId}/recruitments/{sourceRecruitmentId}/entry" },
	"meta": { "requestId": "01900000-0000-7000-8000-000000000003" }
}
```

stale series revision은 `409 series_revision_conflict`다. 동일 key/body는 같은 성공 응답을 replay한다.

### DELETE /api/guilds/{guildId}/series/{id}

물리 삭제가 아니라 series lifecycle 취소 command다. request/권한/lock/revision/confirmed-game reversal은
`POST .../revert`와 동일하다. 성공하면 series는 `CANCELLED` history로 남고 source recruitment는 다시
모집으로 열리지 않도록 `CANCELLED`로 전이한다.

```json
{
	"data": { "nextHref": "/g/{guildId}/series" },
	"meta": { "requestId": "01900000-0000-7000-8000-000000000003" }
}
```

연결된 `series_participants`, audit, outbox 등 보존 row 때문에 physical DELETE/cascade는 금지한다.

### GET /api/guilds/{guildId}/series/{id}/balance.svg

필요 시 현재 result 공유용 SVG를 생성한다. UI 필수 read와 분리한다.

## 경매 내전

### GET /api/guilds/{guildId}/auction-recruitments

`state=actionable`만 지원한다. OPEN 또는 CLOSED이고 아직 tournament로 전환되지 않은 경매 모집을
updatedAt/id 내림차순으로 반환한다. OPEN row의 `href`는
`/g/{guildId}/auction/recruitments/{id}`, CLOSED row의 `href`는
`/g/{guildId}/auction/recruitments/{id}/draft`다. 목록은 Web 생성 직후의 OPEN 모집과 기존 Bot 모집을 같은
projection으로 보여준다.

### POST /api/guilds/{guildId}/auction-recruitments

Discord `/경매내전_10인`·`/경매내전_20인`과 Web `/g/{guildId}/auction`의 경매 모집 dialog가 같은 canonical create command를 쓴다.

Bot request는 service principal, Discord interaction `Idempotency-Key`, actor snapshot, `format`과 rules를 사용한다.
Browser request는 Discord OAuth session, CSRF, browser `Idempotency-Key`, `format`과 rules를 사용하며 actor를
지정할 수 없다.

```json
// Bot
{
  "actor": { "id": "100000000000000001", "globalDisplayName": "Longcat", "displayName": "MSM Longcat", "avatarHash": null, "roleIds": [] },
  "format": "TEN_PERSON",
  "semifinalBestOf": null,
  "finalBestOf": 3,
  "hardFearless": true,
  "expectedStartAt": "2026-07-26T20:00:00+09:00"
}

// Browser
{
  "format": "TWENTY_PERSON",
  "semifinalBestOf": 1,
  "finalBestOf": 5,
  "hardFearless": true,
  "expectedStartAt": "2026-07-26T20:00:00+09:00"
}
```

`format`은 `TEN_PERSON | TWENTY_PERSON`만 허용하고 target count는 각각 10/20으로 server가 파생한다.
`expectedStartAt`은 시간대가 포함된 RFC 3339 timestamp를 받으며, 누락하거나 빈 값이면 server가 수신 시점 30분 후로 정한다. 값을 직접 넣을 때 형식 오류는
`422 invalid_expected_start_at`으로 안전하게 설명한다. 생성 제목의 예상 시작 표기는 KST 24시간
`YYYY-MM-DD HH:mm`과 `(KST)`를 사용한다.
`finalBestOf`와 non-null `semifinalBestOf`는 `1 | 3 | 5`다. 10인은 `semifinalBestOf=null`만 허용하고, 20인은
두 준결승이 공유하는 non-null `semifinalBestOf`와 독립 `finalBestOf`를 사용한다. 생략한 호환 필드는
BO3/하드 피어리스 사용으로 정규화한다.
Bot과 Browser 모두 ACTIVE guild member여야 한다. Backend는 current ACTIVE season과
`AUCTION_RECRUITMENT` guild counter를 lock한 뒤 OPEN `auction_recruitments`, 빈
`auction_setup_drafts(revision=0)`, format별 빈 team slot(2 또는 4)을 한 transaction으로 생성한다.

어느 생성 주체든 `guild_config.recruitment_channel_id`가 반드시 설정되어 있어야 한다. 같은 transaction에
그 채널을 대상으로 `AUCTION_RECRUITMENT_MESSAGE_CREATE` notification을 적재한다. `201 Created`는 DB state와
Discord delivery work가 확정되었음을 뜻할 뿐 Discord 게시 완료를 뜻하지 않는다. 채널이 없으면 전체
transaction을 rollback하고 `409 recruitment_channel_not_configured`를 반환한다. Bot slash command와 Browser는
동일 outbox delivery/typed acknowledgement가 message reference를 bind할 때까지 channel/message ID를 추측하지
않는다.

response는 생성된 `AuctionRecruitmentSnapshot`이며 `recruitment.href`는 OPEN 모집 상세 route다. 동일 principal,
idempotency key, request body는 같은 response를 replay하며 request가 다르면 `409 idempotency_conflict`다.

### GET /api/guilds/{guildId}/auction-recruitments/{id}

ACTIVE guild member에게 authoritative auction recruitment snapshot을 반환한다. response는 다음 구조다.

```text
recruitment
  id, displayNumber, title, format, rules{hardFearless, semifinalBestOf, finalBestOf}
  participantCount, joinedParticipantCount, teamCount
  status, revision, channelId, messageId, createdById, updatedAt, href
participants[] = id, displayName, declaredLane
links
  draft: string | null
permissions
  canManageMembers, canClose, canSendParticipantConfirmationDms, canCancel
```

`draft` link는 CLOSED 모집에만 non-null이다. `canManageMembers`는 OPEN 모집에서 현재 active operator가
browser roster add/remove를 실행할 수 있을 때만 true다. Browser는 권한을 자체 계산하지 않고 permissions를
사용한다.
CANCELLED 또는 CONVERTED 모집은 이 read route에서 `404`다.

### POST /api/guilds/{guildId}/auction-recruitments/{id}/close

Browser session, CSRF, browser `Idempotency-Key`가 필요하다. active operator만 OPEN 모집을 정원(10 또는 20명)
까지 채운 뒤 CLOSED로 전환할 수 있다. transaction은 current recruitment revision/state와 JOINED participant 수를 다시 검증하고,
recruitment revision/audit/realtime invalidate를 함께 기록한다. 성공 `200`은 updated
`AuctionRecruitmentSnapshot`을 반환하며 `links.draft`가 non-null이다. 정원이 부족하면 safe reason
`auction_recruitment_not_full`, stale/invalid lifecycle이면 해당 conflict reason을 반환한다. bound Discord
message가 있으면 같은 transaction에 `AUCTION_RECRUITMENT_MESSAGE_UPDATE`를 적재한다. 마감 자체는 참가 확정
DM을 자동 enqueue하지 않는다.

### POST /api/guilds/{guildId}/auction-recruitments/{id}/participant-confirmation-dms

Browser session, CSRF, browser `Idempotency-Key`, 빈 `{}` body를 사용하는 수동 발송 요청이다. current active
operator만 `CLOSED` 경매 모집에서 호출할 수 있다. canonical `JOINED` Discord 참가자별
`AUCTION_RECRUITMENT_PARTICIPANT_CONFIRMATION_DM_CREATE` notification을 적재하며, 같은 CLOSED revision·recipient
조합은 한 번만 적재된다. 응답은 `queuedRecipientCount`와 `alreadyQueuedRecipientCount`다. 더미 사용자는
제외하고, DM delivery 실패는 notification failure로 기록하며 모집 상태는 유지한다.

### DELETE /api/guilds/{guildId}/auction-recruitments/{id}

Browser session, CSRF, browser `Idempotency-Key`를 사용하는 bodyless soft-cancel command다. OPEN 모집을
생성자 또는 active operator가 CANCELLED로 전이할 수 있다. participant/setup draft/team slot/history는
보존하고 audit/realtime invalidate를 같은 transaction에 기록한다. 동일 idempotency key는 cancel snapshot을
replay하며 새 key의 재취소는 lifecycle conflict다. bound Discord message가 있으면 같은 transaction에
`AUCTION_RECRUITMENT_MESSAGE_DELETE`를 적재한다.

### POST /api/guilds/{guildId}/auction-recruitments/{id}/members

Bot과 Browser가 같은 canonical roster command를 사용한다.

- Bot service principal은 Discord interaction `Idempotency-Key`와
  `{ actor, member?, declaredLane? }`를 보낸다. `member`를 생략하면 actor 자신의 ACTIVE 길드 소속을
  확보한 뒤 OPEN 모집에 self-service로 참가한다. `member`를 지정하면 actor는 configured operator여야 하고,
  backend가 대상 actor snapshot을 동기화한 뒤 대상의 ACTIVE 길드 소속을 검증해 참가시킨다. lane을 생략하면
  참가 대상의 저장된 guild main lane을 사용하고, 둘 다 없으면
  `422 auction_declared_lane_required`를 반환한다.
- Browser는 Discord OAuth session, CSRF, browser `Idempotency-Key`와
  `{ memberId, declaredLane }`를 보낸다. 현재 active operator만 existing ACTIVE guild member를 추가할 수
  있다. `memberId`는 opaque stored user ID이므로 Discord user와 development dummy user 모두 같은 경로로
  처리하며, browser는 declared lane을 명시해야 한다.

선택 lane의 현재 시즌 MMR은 1500 기본값을 포함해 `CAPTURED` snapshot으로 저장한다. capacity/duplicate/state를
transaction에서 검증하고, recruitment/setup draft revision, audit, realtime outbox, bound Discord message
update를 함께 기록한다.

### DELETE /api/guilds/{guildId}/auction-recruitments/{id}/members/{userId}

Bot과 Browser가 같은 canonical roster command를 사용한다.

- Bot service principal은 Discord interaction `Idempotency-Key`와 `{ actor }`를 보낸다. actor는 자신을
  취소하거나 configured operator role로 다른 참가자를 제거할 수 있다.
- Browser는 Discord OAuth session, CSRF, browser `Idempotency-Key`와 빈 `{}` body를 보낸다. 현재 active
  operator만 path의 참가자를 제거할 수 있다.

OPEN 모집에서만 허용하며, 참가 row를 `WITHDRAWN`으로 전환하고 setup team에 남아 있던 captain 참조를 같은
transaction에서 비운다. recruitment/setup draft revision, audit, realtime outbox와 bound Discord message
update도 같은 transaction에 기록한다.

### POST /api/guilds/{guildId}/auction-recruitments/{id}/force-cancel

BotBearer 전용의 operator recovery command다. body는 `{ actor }`, `Idempotency-Key`는 Discord interaction
ID다. browser lifecycle action이나 Discord 모집 카드 component가 이 endpoint를 호출하지 않는다.

- backend는 actor의 ACTIVE guild membership 및 configured operator role을 다시 검증한다. 동일 interaction
  ID와 동일 body는 stored result를 replay하고, 다른 body는 `409 idempotency_conflict`다.
- `OPEN` 또는 `CLOSED` 모집은 participant, setup draft, team slot과 history를 유지한 채 모집만
  `CANCELLED`로 전환한다. bound Discord 모집 메시지는 typed DELETE outbox로 제거한다.
- `CONVERTED` 모집은 linked tournament를 해석해 기존 tournament cancellation transaction으로 위임한다.
  `BIDDING`, `BRACKET_SETUP`, `IN_GAME`만 취소 가능하며 active match, tournament, source recruitment은 함께
  `CANCELLED`가 된다. `COMPLETED` 또는 이미 `CANCELLED`인 tournament는 lifecycle conflict다.
- confirmed game이 있으면 latest confirmed time 순으로 MMR rollback을 수행한다. 각 ledger가 global MMR
  history tail인지 검증하므로 더 최근의 MMR mutation이 있으면 부분 취소 없이
  `409 auction_tournament_cancel_mmr_history_advanced`로 전체 transaction을 rollback한다. immutable
  bid/action/point/game history는 삭제하지 않는다.
- audit와 tournament/recruitment realtime invalidation은 같은 transaction에 기록한다.

### GET/PATCH /api/guilds/{guildId}/auction-recruitments/{id}/setup-draft

`GET`은 ACTIVE 길드 멤버에게 CLOSED 모집의 setup draft와 JOINED roster를 반환한다. `PATCH`는
browser session, CSRF, browser `Idempotency-Key`를 요구하며 current active operator만 실행할 수 있다.
body는 `expectedRevision`과 format에 맞는 순서 팀 배열이다. 각 팀은 `teamIndex`, `teamName`,
`captainUserId`, `initialPoints`를 포함한다.

PATCH는 모집을 변경하지 않고 draft/팀 설정 revision, audit, outbox만 갱신한다. captain은 정확히 한 번씩
선택된 JOINED participant여야 한다. 10인은 2팀, 20인은 4팀을 정확히 제공해야 하며, draft revision 충돌은
`409 auction_setup_draft_revision_conflict`로 현재 revision을 함께 반환한다.

### POST /api/guilds/{guildId}/auction-tournaments

browser session operator가 CLOSED/full 모집과 complete setup draft에서 경매 내전을 시작한다. request는
`sourceRecruitmentId`, `expectedRevision`이며 CSRF와 browser `Idempotency-Key`가 필수다. 생성 transaction은
source recruitment, setup draft, setup team, JOINED participant를 잠그고 source를 CONVERTED로 전이한다.
동시에 immutable rule snapshot, captain roster, initial point ledger, non-captain candidate queue를 만들고
`BIDDING` snapshot을 반환한다. source recruitment의 기존 active tournament는 `409`로 거절한다.

### GET /api/guilds/{guildId}/auction-tournaments

query는 `state=active|completed`, 선택 `seasonId`, `cursor`, `limit`이다. `seasonId`가 있으면 그 canonical
season UUID의 tournament만 반환한다.

`active`는 `BIDDING`, `BRACKET_SETUP`, `IN_GAME`만, `completed`는 `COMPLETED`와 `CANCELLED`를 함께 반환한다.
취소된 row는 결과 확정이 아닌 archive로 표시하지만, 기존 결과 snapshot URL은 계속 읽을 수 있다. Browser
session은 ACTIVE 길드 소속을 검증한 뒤 viewer-scoped list를 받고, BotBearer는 service principal branch로
같은 canonical projection을 읽는다. Bot list branch는 browser session이나 viewer identity를 요구하지 않는다.

### GET /api/guilds/{guildId}/auction-tournaments/{id}

ACTIVE 길드 멤버가 canonical draft aggregate를 읽는다. 응답은 tournament immutable rule snapshot, team별
현재 point/roster, 전체 candidate queue, current target, UNSOLD 후보, 마지막 revert 가능한 action, viewer
permission을 포함한다. current target이 있으면 그 대상의 Discord/Riot 아이콘, cached Riot tier, 해당 tournament
season의 내전 요약/라인 MMR, 신청 라인 선호 챔피언, 확정된 내전 주력 챔피언과 최근 게임, 식별자를 제외한
cached Riot 요약도 `currentTargetProfile`로 함께 반환한다. Riot cache가 없거나 유효하지 않으면 해당 Riot
영역만 `null`이며 provider refresh를 발생시키지 않는다. 이 정보는 화면 조회용이며 입찰 판정은 항상 candidate의
immutable `mmrAtStart`만 사용한다. `href`는
`/g/{guildId}/auction/tournaments/{tournamentId}/draft`다. `canManageDraft`는
`BIDDING` 상태의 현재 active operator에게만 true다. `canRevertLast`는 가장 최근 action이 직접 draft action이고 server
검증상 되돌릴 수 있고 active bracket artifact가 없을 때만 true다. 이 capability는 `BIDDING`과 아직 lock하지
않은 `BRACKET_SETUP`에서만 반환되며, client는 action history나 화면 상태로 eligibility를 추정하지 않는다.

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

browser-only command route다. current active operator, CSRF, browser `Idempotency-Key`,
`expectedRevision`이 모두 필요하다. command는 tournament -> teams -> active team members -> candidates ->
lots의 mutable row를 잠근다. append-only action/point ledger는 tournament lock 아래서 읽고,
action/audit/outbox와 snapshot을 같은 transaction에서 기록한다.

- `DRAW_NEXT`는 가장 앞 QUEUED candidate를 ACTIVE로 만들고 drawCount를 증가시킨다.
- `MARK_UNSOLD`, `AWARD_BID`, `ASSIGN_CURRENT`는 현재 target과 일치해야 한다. AWARD_BID는 team point를
  차감하고 BID roster/point ledger를 같이 만든다.
- `ASSIGN_UNSOLD`는 active target이 없는 UNSOLD candidate만 manual roster로 옮긴다.
- `RESTORE_UNSOLD`는 active target이 없을 때 유찰 후보에 새 ACTIVE lot을 만들며 queue 뒤로 보내거나
  drawCount를 증가시키지 않는다. 이전 UNSOLD 또는 reverted assignment lot은 history로만 보존한다.
- `REVERT_LAST`는 action ledger를 수정하지 않고 새 reversal action을 append한다. `MARK_UNSOLD`,
  `AWARD_BID`, `ASSIGN_CURRENT` rollback은 새 ACTIVE lot으로 즉시 current target을 복원한다.
  `ASSIGN_UNSOLD` rollback은 candidate를 UNSOLD로만 되돌린다. 낙찰 rollback은 roster release와 reversal
  point ledger를 함께 기록한다.

대진 slot이 아직 unlock인 `BRACKET_SETUP`에서는 `REVERT_LAST`만 가능하며, transaction 안에서 모든
`auction_bracket_slots.team_id`를 먼저 비우고 tournament를 `BIDDING`으로 되돌린다. 이 rollback은 최근
직접 draft action과 그 roster/point ledger만 되돌리며 source recruitment를 다시 열지 않는다. match, lineup,
game artifact 또는 locked bracket이 있으면 draft mutation은 `409 auction_tournament_draft_locked` 또는 state
conflict로 거절된다.

### GET /api/guilds/{guildId}/auction-tournaments/{id}/cancel-preview

BotBearer 전용 read endpoint다. Discord `/경매조기종료`는 여기서 canonical tournament UUID, display number,
title, 현재 status와 revision을 읽고 POST cancel의 `expectedRevision`에 사용한다. body와 idempotency key는
필요 없다. 이 read는 terminal status도 반환하지만 권한·상태·revision의 최종 판정은 반드시 다음 POST
transaction에서 다시 수행한다.

### POST /api/guilds/{guildId}/auction-tournaments/{id}/cancel

Bot service principal만 호출하는 operator 조기 종료 command다. browser action은 제공하지 않으며, 실제 Discord
slash command/client가 이 endpoint를 호출해야 사용자 기능으로 도달한다. `Idempotency-Key`, `expectedRevision`,
non-empty reason, actor snapshot과 audit가 필요하다.

- `BIDDING`, `BRACKET_SETUP`, `IN_GAME`만 취소할 수 있다. `COMPLETED`와 `CANCELLED`는 `409`다.
- `IN_GAME`은 tournament의 active match에 속한 모든 confirmed game을 confirmed time 역순으로 되돌린다.
  각 game의 MMR ledger가 전역 MMR history tail이어야 하며, 이후 MMR 변경이 있으면 일부만 취소하지 않고
  transaction 전체를 `409 auction_tournament_cancel_mmr_history_advanced`로 rollback한다.
- game rollback이 끝난 뒤 active match, tournament, source recruitment을 모두 `CANCELLED`로 전환한다.
  source recruitment의 bound Discord 모집 메시지는 typed DELETE outbox로 제거하고 tournament/recruitment
  invalidation outbox와 audit를 같은 transaction에 기록한다.

### GET /api/guilds/{guildId}/auction-tournaments/{id}/bracket

활성 길드 멤버가 읽는 authoritative bracket aggregate다. tournament, team roster, 고정 순서의
`A_0`, `A_1`, `B_0`, `B_1` slot, 생성된 match의 score 요약, viewer 권한을 함께 반환한다.
`rules.semifinalBestOf`는 10인에서는 명시적으로 `null`, 20인에서는 1/3/5 숫자다.

- `BIDDING`에서는 `409 auction_tournament_bracket_unavailable`을 반환한다. `BRACKET_SETUP`, `IN_GAME`,
  `COMPLETED`, `CANCELLED`는 bracket history를 읽을 수 있다.
- `permissions.canRevertDraft`는 active operator가 아직 unlock인 `BRACKET_SETUP`에 있고 active match가
  없으며 server가 마지막 직접 draft action의 rollback을 허용할 때만 true다. true일 때 Web은 기존 draft
  command route의 `REVERT_LAST`를 사용한다. 이 command는 slot을 비우고 `BIDDING`으로 돌아가지만 source
  recruitment를 다시 열지는 않는다.

### POST /api/guilds/{guildId}/auction-tournaments/{id}/bracket/commands

```text
ASSIGN_TEAM
CLEAR_SLOT
LOCK_BRACKET
UNLOCK_BRACKET
```

browser-only active operator command다. CSRF, browser `Idempotency-Key`, `expectedRevision`가 모두
필요하며 tournament -> team -> active roster -> slot -> match 순서로 lock하고 audit/outbox와
snapshot을 같은 transaction에 기록한다. `BRACKET_SETUP`이면서 아직 unlock인 경우에만 바꿀 수 있다.

- `ASSIGN_TEAM`은 이전 배치와 target slot을 먼저 비운 뒤 새 slot에 넣는다. 따라서 같은 팀은 한 slot에만
  존재한다.
- 10인은 `A_0`, `A_1`만 허용한다. `LOCK_BRACKET`은 각 팀의 active roster 5명과 두 slot을 검증한 뒤,
  configured `finalBestOf`와 `hardFearless`를 가진 `FINAL` 하나를 만든다.
- 20인은 네 slot 모두를 사용한다. `LOCK_BRACKET`은 active roster 5명인 네 팀과 네 slot을 검증한 뒤,
  configured `semifinalBestOf`의 `SEMIFINAL_A`, `SEMIFINAL_B` 및 configured `finalBestOf`의 빈
  `FINAL`을 만든다. 모든 match는 tournament의 `hardFearless`를 복사한다.
- bracket lock은 tournament를 `BRACKET_SETUP`에 유지한다. lineup lock 이후 match 진행 flow가
  tournament를 `IN_GAME`으로 전환한다.
- `UNLOCK_BRACKET`은 lock된 `BRACKET_SETUP` 또는 `IN_GAME` bracket의 recovery command다. 생성된 모든
  match에 `games`와 `game_inputs`가 하나도 없을 때만 허용하며, 기존 match를 `CANCELLED` history로 남긴 뒤
  bracket lock을 풀고 slot 편집 상태로 복원한다. 취소된 이전 match/lineup은 새 bracket lock의 artifact가 아니다.

### GET /api/guilds/{guildId}/auction-matches/{matchId}

`match`, 양 팀 roster, `lineups`, 확정 `games`, viewer `permissions`를 한 snapshot으로 반환한다.
각 game의 `picks` key는 반드시 잠긴 lineup의 10개 user ID와 정확히 일치하며, pick/ban은 null일 수 있다.
`leftScore`/`rightScore`는 확정 game의 side preset과 winner에서만 계산한다.
20인 `SEMIFINAL_A/B`는 두 준결승 lineup이 모두 lock되기 전까지 `permissions.canEnterResults=false`를
반환한다. 이 projection은 화면 제어용이며 game submit endpoint도 같은 조건을 transaction 안에서 다시
검증한다. `canRevertLastGame`도 command와 같은 조건을 사용한다. 완료된 20인 준결승은 final의 다른 팀
라인업, game, lineup lock 또는 `LINEUP_PENDING` 이외의 상태가 있으면 false이며, client가 보이는 control과
실제 command 결과가 어긋나지 않는다.

### PATCH /api/guilds/{guildId}/auction-matches/{matchId}/lineup

한 선수를 한 lane에 배정/해제한다. 전체 lineup replace를 반복하지 않는다.

### POST /api/guilds/{guildId}/auction-matches/{matchId}/lineup/lock

양 팀 TOP/JUNGLE/MID/BOTTOM/SUPPORT가 정확히 한 명씩이고 team membership과
중복이 유효해야 한다. `guild_config.entry_channel_id`가 설정되어 있으면 성공 transaction에
`AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE` notification도 함께 적재한다. payload는 tournament/match rule,
양 팀의 lane-ordered 5인 roster와 match deep link를 immutable snapshot으로 고정하고, Bot relay는 이를
entry channel에 게시한 뒤 generic acknowledgement를 보낸다. entry channel이 `NULL`이면 lock은 정상
성공하고 notification만 생략한다. lineup을 unlock해도 이미 게시한 카드를 삭제하거나 수정하지 않으며,
변경 후 다시 lock하면 해당 match의 새 revision을 deduplication key에 사용해 새 confirmation 카드를 적재한다.

### POST /api/guilds/{guildId}/auction-matches/{matchId}/lineup/unlock

operator recovery command다. lock metadata가 모두 있는 `READY` match만 다시 열 수 있으며, 해당 match의
`games` 또는 `game_inputs` row가 하나라도 있으면, voided/superseded history를 포함해 거절한다. 기존 lane
assignment는 유지하고 lock metadata만 비운 뒤 match를 `LINEUP_PENDING`으로 되돌린다. UI는 서버의
`permissions.canUnlockLineup`이 true일 때만 이 command를 노출한다.

### POST /api/guilds/{guildId}/auction-matches/{matchId}/games

`expectedRevision`, `setNumber`, side preset, winner, 10명 전체의 pick map, 양 진영 5칸 ban, patch version을
받는다. 별도 tournament revision은 받지 않는다. 서버는 tournament -> match -> locked lineup 순서로 lock한 뒤
다음 set만 허용하고, set 안의 pick/ban 중복 및 같은 match 안의 hard fearless 과거 pick 재사용을 거절한다.
lock 전 입력은 거부한다. 경매 game은 일반 game과 같은 guild MMR service/ledger를 사용한다.
20인 `SEMIFINAL_A/B`는 두 준결승의 10인 lineup이 모두 lock되기 전에는
`409 auction_match_semifinal_lineups_incomplete`로 거절한다.

첫 확정 game은 match를 `IN_PROGRESS`로 전환한다. BO 승점에 도달하면 match를 `COMPLETED`로 전환하고,
20인 `SEMIFINAL_A/B` 승자는 같은 transaction에서 `FINAL`의 left/right team으로 전파한다. 전파 시 해당
준결승의 잠긴 5인 lineup도 final 쪽에 복사하지만 final lock metadata는 두 semifinal이 모두 준비될 때까지
비워 둔다. final 승자는 tournament를 `COMPLETED`로 전환하고, result channel이 설정된 경우에만
`AUCTION_TOURNAMENT_RESULT_CREATE` outbox를 enqueue한다.

### DELETE /api/guilds/{guildId}/auction-matches/{matchId}/games/last

가장 최근 확정 game, MMR ledger, match score/state를 transaction으로 복구한다. source game input은
`SUPERSEDED`가 되어 새 set 입력을 막지 않는다. MMR tail-safe 조건을 만족하지 않으면 거절한다.
완료된 20인 준결승을 되돌릴 때 final의 lineup, game 또는 완료 artifact가 있으면 거절하며, 없을 때만
전파한 final side와 5인 lineup을 함께 제거한다. 완료 final을 되돌리면 tournament는 `IN_GAME`으로 열린다.
완료 final의 결과 Discord card는 같은 transaction에서 reconciliation한다. 아직 delivery를 시도하지 않은
CREATE row만 terminal cancel할 수 있고, delivered CREATE는 typed result DELETE outbox를 enqueue한다. active
lease 또는 attempted-but-undelivered/unknown delivery row가 있으면 card 상태를 안전하게 판단할 수 없으므로
revert를 `409`로 거절한다.

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

`IN_GAME`, `COMPLETED`, `CANCELLED` tournament의 authoritative 집계다. `CANCELLED`는 BIDDING 중 취소되어
match가 0개일 수 있고, bracket recovery의 이전 CANCELLED match history가 여러 개일 수 있다. archive는
canonical team/match history를 반환하되 result rollback 권한은 항상 false이며, 취소 전 confirmed game은
cancellation transaction에서 MMR과 함께 void되어 score/delta에 포함되지 않는다. 일반 참가자에게는 read-only이고,
operator의 결과 되돌리기는 각 canonical match의 `DELETE .../games/last`만 사용한다. 별도 checkpoint API는 없다.

`permissions.canRecoverBracket`은 active operator, lock된 bracket, 생성된 active match가 있고 games와
game_inputs artifact가 전혀 없는 `IN_GAME` tournament에서만 true다. true일 때 Web은 별도 result mutation을
만들지 않고 기존 `POST .../bracket/commands`의 `UNLOCK_BRACKET`을 사용한다. 성공하면 생성 match는
`CANCELLED` history로 보존되고 tournament는 `BRACKET_SETUP` 편집 상태로 돌아간다.

## 길드 전적과 랭킹

### GET /api/guilds/{guildId}/records/suggestions

navbar 내전 검색 자동완성이다. 로그인한 사용자가 현재 active guild의 active member일 때만 사용할 수
있으며, 접근 권한이 없으면 guild 존재 여부를 숨기는 `404`를 반환한다.

```text
q=탐솔&limit=8
```

`q`는 trim 후 2자 이상이어야 하고 `limit`은 기본값과 최댓값이 모두 8이다. active member의 guild
display name을 대소문자 구분 없이 prefix 검색한다. 연결된 main Riot ID와 profile icon은 해당 사용자가
`show_riot_ids=true`로 허용한 경우에만 description과 icon으로 포함한다.

### GET /api/guilds/{guildId}/records/profile

길드 내전 전적 화면의 사용자 프로필 projection이다. Session이 필요하며 요청자와 대상 모두 현재 active
guild의 active member여야 한다. 요청자가 접근할 수 없는 guild와 찾을 수 없는 대상은 tenant 존재 여부를
구분하지 않는 `404 not_found`로 반환한다.

```text
q=Longcat
```

`q`는 trim 후 Unicode scalar 기준 1..100자다. 다음 세 식별자를 exact match한다.

- 올바른 Discord snowflake 또는 `dmy_<UUIDv7>` opaque UserId면 `guild_members.user_id`와 정확히
  일치하는 대상만 찾는다.
- 그 외 값은 guild display name을 대소문자 구분 없이 정확히 일치시킨다.
- `GameName#TagLine`은 `normalize_riot_id_v1`과 같은 NFKC, trim, Unicode case-fold를 적용해 정확히
  일치시킨다. 이 조회 분기는 대상의 `show_riot_ids=true`인 연결 계정에만 접근한다.

같은 display name으로 둘 이상이 일치하면 임의의 첫 row를 선택하지 않고 `409
ambiguous_guild_member`를 반환한다. 공백이거나 100자를 넘는 검색어는 `400 invalid_search_query`다.

```json
{
	"data": {
		"member": {
			"displayName": "Longcat",
			"avatarUrl": "https://cdn.discordapp.com/avatars/...",
			"mainLane": "MID"
		},
		"championPreferences": {
			"TOP": ["Ornn"],
			"JUNGLE": [],
			"MID": ["Ahri", "Orianna"],
			"BOTTOM": ["Ezreal"],
			"SUPPORT": []
		},
		"riotAccounts": [
			{
				"platform": "KR",
				"gameName": "Longcat",
				"tagLine": "KR1",
				"profileIconId": 588,
				"isMain": true
			}
		]
	},
	"meta": { "requestId": "..." }
}
```

`championPreferences`는 season 통계가 아니라 global user가 지정한 순서를 보존한 다섯 lane 전체
snapshot이다. `show_riot_ids=true`면 연결된 모든 Riot 계정을 main 우선으로 반환하고, false거나 privacy
row가 없거나 연결 계정이 없으면 `riotAccounts`는 빈 배열이다. Browser가 privacy flag를 받아 다시
판단하지 않는다. `member.profileHref`는 대상이 Discord identity이고 `public_profile_enabled=true`일 때만
`/users/{discordId}`를 반환하며, 비공개 사용자와 DUMMY identity에는 `null`이다. Browser는 이 nullable
canonical href를 그대로 사용하고 Discord ID를 조합하거나 privacy를 재판단하지 않는다. 응답에는 Discord
user ID를 독립 field로 넣지 않으며 PUUID, routing region과 계정 소유 내부 key도 반환하지 않는다.
`avatarUrl`은 Discord CDN provider URL이므로 URL path에는 provider가 요구하는 Discord ID가 포함될 수 있다.

`show_riot_ids`는 Discord 사용자와 Riot 계정의 연결 관계 공개만 제어한다. `/api/riot/records`와
`/api/riot/records/suggestions`에서 Riot ID 자체를 검색하는 기능에는 적용하지 않는다.

### GET /api/guilds/{guildId}/records/search

길드 내전 전적 화면 전체를 구성하는 read projection이다. Session이 필요하며 `profile`과 같은 exact identity
resolver를 사용한다. 요청자와 대상은 모두 현재 active guild의 active member여야 한다. 접근할 수 없는
guild와 inactive/missing target은 `404 not_found`, 현재 guild에 없거나 존재하지 않는 season은 `404
season_not_found`다. 두 season 경우는 같은 code를 사용해 foreign season 존재 여부를 구분하지 않는다. 공백
또는 100자를 넘는 `q`와 잘못된 cursor/limit은 `400`이고, 중복 display name은 `409
ambiguous_guild_member`다.

```text
q         required, Discord ID | guild display name | 공개된 full Riot ID exact match
seasonId optional UUID
cursor   optional opaque URL-safe keyset cursor
limit    optional integer, default 20, 1..50
```

`type`과 `lane`은 query parameter가 아니다. 현재 화면의 일반/경매와 lane control은 이미 읽은 page 안에서만
적용하는 frontend local filter다. Pagination 자체는 filter와 무관한 전체 record stream을 사용한다.

`seasonId`가 없으면 ACTIVE season을 선택한다. ACTIVE season이 없으면 종료 season을 임의로 선택하지 않고
`selectedSeasonId=null`, summary zero, baseline lane 다섯 개와 빈 champion/record 배열을 반환한다. 명시한
season은 CLOSED도 선택할 수 있지만 반드시 현재 guild 소유여야 한다. `seasons`는 ACTIVE 우선, 이후
CLOSED를 `startedAt DESC`로 반환한다.

응답 핵심 shape:

```text
member, championPreferences, riotAccounts
seasons[] = id, name, status, startedAt, endedAt
selectedSeasonId
summary = games, wins, losses
laneMmr[5] = lane, mmr, games, wins
championRecords[] = championId, lane, games, wins, losses
records[] = SERIES | TOURNAMENT
nextCursor
```

`member`, `championPreferences`, `riotAccounts`는 profile endpoint와 같은 projection이다. 따라서
`show_riot_ids=true`인 연결 계정만 main 우선으로 반환하고, false 또는 privacy row 부재면 빈 배열이다.
`show_riot_ids`는 Riot ID로 target을 resolve하고 Discord-Riot 연결 관계를 표시하는 범위만 제어한다. 같은
guild의 내부 game/MMR/champion 통계는 개인 privacy 설정으로 숨기지 않는다. `member.profileHref`는
`public_profile_enabled=true`인 Discord identity만 가진 nullable canonical route이며, UI의 대상 이름은 이
값이 있을 때만 `/users/[discordId]`로 연결한다. Discord user ID는 독립 field로 반환하지 않으며 PUUID,
routing region, normalized Riot ID는 넣지 않는다.

`laneMmr`는 선택 season에 대해 `TOP, JUNGLE, MID, BOTTOM, SUPPORT` 다섯 row를 항상 canonical 순서로
반환한다. 아직 `user_lane_mmr` row가 없는 lane은 저장 row를 만들지 않고 `mmr=1500`, `games=0`,
`wins=0`으로 projection한다. 기존 목업의 의미가 불명확한 `change`는 API field로 만들지 않는다.
`championRecords`는 confirmed game의 `championId + lane` 집계이며 games 내림차순, wins 내림차순,
championId 오름차순, canonical lane 순으로 정렬한다. Champion이 기록되지 않은 game은 champion 집계에서
제외하지만 전체 승패에는 포함한다.

`SERIES`는 완료된 일반 series와 10인 경매의 완료 final을 같은 10인 row 구조로 반환한다. `source`가
`REGULAR|AUCTION`을 구분하고, title/result/team/opponent/stage/set 승패/총 MMR delta와 confirmed game
detail을 포함한다. `TOURNAMENT`는 20인 경매 한 개를 outer row 하나로 묶고 team, 사용 point, placement,
roster와 참가한 `SEMIFINAL_A|SEMIFINAL_B|FINAL` series를 내부에 둔다. 20인 내부 series는 별도 outer
page row로 중복하지 않는다.

각 game detail은 `gameNumber`, `playedAt`, `winningSide`, target의 side/lane/champion/MMR before/after,
blue/red lineup과 ban을 제공한다. `championId`는 신뢰 가능한 pick이 없으면 null이고, legacy
`LEGACY_NOT_APPLIED`처럼 target `GAME_RESULT` ledger가 없으면 game MMR before/after와 상위 `mmrDelta`는
null이다. 한국어 결과/경매 단계/상대 시간과 score 문자열은 frontend가 canonical enum, timestamp,
wins/losses로 만든다.

집계와 detail은 `games.status='CONFIRMED'`만 사용하며 cancelled/deleted parent와 VOIDED game을 제외한다.
각 outer row와 20인 내부 series의 `playedAt`은 그 scope에서 target이 참가한 최신 confirmed
`games.played_at`이다. Outer record는 `(playedAt DESC, id DESC, kind DESC)`로 정렬한다. Cursor는 마지막
`playedAt,id,kind`를 담은 canonical URL-safe opaque token이며 같은 `q`와 season selection에서만 재사용한다.
`limit + 1` keyset pagination으로 `nextCursor`를 만들고 20인 tournament grouping 경계를 쪼개지 않는다.

대상이 존재하지만 선택 season에 확정 기록이 없으면 오류가 아니라 summary zero, baseline lane 다섯 개,
빈 champion/record 배열과 `nextCursor=null`을 포함한 `200`이다. 내부 `games`와 append-only MMR ledger가
source of truth이며 별도 records summary table을 두지 않는다.

### GET /api/guilds/{guildId}/leaderboard

로그인한 사용자가 ACTIVE guild의 ACTIVE member일 때만 조회한다. 접근할 수 없는 guild와 비활성 member는
guild 존재 여부를 숨기는 `404`를 반환한다. Query parameter 없이 ACTIVE season만 조회하며, 활성 season이
없으면 오류 대신 다음 `200`을 반환한다.

```json
{
	"data": { "season": null, "players": [] },
	"meta": { "requestId": "..." }
}
```

활성 season이 있으면 frontend가 통합/라인별 순위를 만들 수 있는 전체 player snapshot을 반환한다.

```text
season = id, name
players[] = id, name, tag|null, profileIconId|null, compositeMmr, mainRole, isMe
players[].stats[TOP|JUNGLE|MID|BOTTOM|SUPPORT] = mmr, games, wins, championId|null
```

`players`는 ACTIVE member 중 해당 season의 `user_lane_mmr.games_played >= 1`인 lane이 하나 이상 있는
사용자만 포함한다. `stats`는 실제 플레이한 canonical lane만 key로 갖는 partial object다.
`compositeMmr`는 반올림하지 않은
`SUM(laneMmr * gamesPlayed) / SUM(gamesPlayed)`이고, API가 반환한 모든 lane을 사용한다. `mainRole`은
member의 설정 `main_lane`이 실제 플레이한 lane이면 이를 사용한다. 그렇지 않으면 games 내림차순, MMR
내림차순, `TOP, JUNGLE, MID, BOTTOM, SUPPORT` 순으로 하나를 결정한다.

각 lane의 `championId`는 confirmed game 중 취소/삭제되지 않은 일반 series 또는 경매 match/tournament에
속한 기록만 집계한 최다 플레이 champion이다. 동률은 champion ID 오름차순으로 결정한다. `tag`는 main
Riot ID 문자열이고 `profileIconId`와 함께 대상 사용자의 `show_riot_ids=true`일 때만 반환한다. 이 privacy
설정은 guild 내부 MMR/승패를 숨기지 않는다.

통합/라인 탭, player 검색, 고정 25명 단위 local pagination, 순위와 승률은 frontend가 받은 snapshot에서
계산한다. 순위는 page-relative가 아니라 전체 정렬 기준이다. 이 endpoint는 pagination/filter query와
WS를 사용하지 않는다.

## Riot 외부 전적

### GET /api/riot/records/suggestions

navbar Riot 검색 자동완성용 public route다.

```text
q=Longcat&platform=KR&limit=8
```

`q`는 기존 Riot ID normalization을 적용한 뒤 2자 이상이어야 한다. `platform` 기본값은 `KR`이고
`limit`은 기본값과 최댓값이 모두 8이다. Riot provider는 호출하지 않고 `riot_accounts`와
`riot_records_snapshots`에 이미 알려진 canonical Riot identity를 prefix 검색한다. Riot ID 공개 검색에는
사용자 privacy flag를 적용하지 않으며, 응답에 Discord 사용자 연결 정보와 PUUID를 포함하지 않는다.

두 suggestion route는 같은 success envelope를 사용한다.

```json
{
	"data": {
		"suggestions": [
			{
				"kind": "riotAccount",
				"value": "Longcat#KR1",
				"label": "Longcat#KR1",
				"description": "KR",
				"profileIconId": 29
			}
		]
	},
	"meta": { "requestId": "..." }
}
```

`kind`는 `riotAccount` 또는 `guildMember`이고 nullable field는 `description`, `profileIconId`다. 결과가
없으면 `suggestions`는 빈 배열이다.

### GET /api/riot/records

query:

```text
gameName=Longcat
tagLine=KR1
platform=KR
```

`platform`은 Riot platform routing 값이며 MVP는 `KR`만 지원한다. Client는 `ASIA` 같은 regional
routing 값을 보내지 않는다. Backend가 platform을 regional routing으로 변환하고, 공통 Riot ID
resolver로 Account-v1을 호출해 canonical `gameName`, `tagLine`, PUUID를 얻는다.

Public/browser route와 durable records cache의 owner는 Axum이다. Axum은 private Riot integration service에
internal bearer token으로 provider-neutral request를 보내며 Riot developer key를 보유하지 않는다.
Integration service는 Riot key, provider 호출과 변환만 책임지고 MonkeyBot session, user, guild,
PostgreSQL에 접근하지 않는다.

응답은 Riot provider payload를 그대로 노출하지 않는 records page DTO다. Profile, rank, 조회 구간의
champion aggregate, recent match summary/detail과 cache 상태를 success envelope의 `data`로 반환한다.
LP 변화량처럼 Riot API로 확인할 수 없는 값은 만들지 않는다. 조회는 cache-backed이며 stale cache를
반환한 경우에도 `cache.state=stale`로 명시한다. 새 provider 조회는 `fresh`, 명시 refresh 성공은
`refreshed`다.

`championAggregates`는 `MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS` 이후 ranked match ID를 100개씩
탐색해 계산하며 최근 경기 10개 표본과 분리한다. 최대 2,000경기까지 탐색하고 그 이상이면
`sample.championAggregates.truncated=true`로 명시한다. 기본 정렬은 게임 수 내림차순, 승수 내림차순,
챔피언 이름 오름차순이다.

### GET /api/riot/records/matches

```text
gameName=Longcat&tagLine=KR1&platform=KR&offset=10&count=10
```

최근 경기 더보기 전용 public route다. `count`는 1..20, `offset`은 0..10,000이며 모든 queue를 최근순으로
반환한다. 응답 `page`는 `offset`, `requested`, `returned`, `hasMore`, `nextOffset`을 포함한다. 정확한
`(platform, puuid, offset, count)` 응답은 PostgreSQL에 저장해 같은 구간 재조회 시 Riot provider를 다시
호출하지 않는다. 명시 refresh가 성공하면 match ordering 변경을 반영하기 위해 해당 page cache를 삭제한다.

### POST /api/riot/records/refresh

```json
{
	"gameName": "Longcat",
	"tagLine": "KR1",
	"platform": "KR"
}
```

로그인한 사용자의 명시적 refresh다. Session, CSRF와 별도 Riot refresh rate limit을 적용하며 GET과
같은 records page DTO를 반환한다. 마지막 성공 갱신부터 30초 동안은 PostgreSQL refresh gate가 재호출을
거부하고 남은 시간을 반환한다. 같은 Riot ID의 동시 갱신은 하나의 5분 lease만 provider를 호출한다. Riot
429/timeout에서는 저장된 snapshot을 변경하지 않고 stale cache로 반환하며, 없으면 normalized error
envelope를 반환한다.

조회와 refresh, 아래 계정 연결은 같은 `resolve_riot_id(platform, gameName, tagLine)` application
service를 사용한다. Handler나 frontend가 `GameName#TagLine`을 다시 파싱하거나 PUUID를 조립하지 않는다.

### Private Axum -> Riot integration contract

Local base URL은 `http://127.0.0.1:30040/internal/riot/`, dev container base URL은
`http://riot-api-dev:8081/internal/riot/`다. 다음 relative path만 사용한다.

```text
POST accounts/resolve
GET  records?gameName={name}&tagLine={tag}&platform=KR
GET  records/matches?gameName={name}&tagLine={tag}&platform=KR&start={n}&count={1..20}
POST records/refresh
```

`accounts/resolve`는 `platform`과 tagged `lookup`을 받는다. 사용자 입력으로 연결할 때는
`{ "kind": "riotId", "gameName", "tagLine" }`, 저장된 account를 갱신할 때만
`{ "kind": "puuid", "puuid" }`를 쓴다. Records refresh body는 public refresh와 같은
`{ gameName, tagLine, platform }`다.

모든 internal request는 `Authorization: Bearer <internal-token>`을 요구한다. Axum의
`MONKEYBOT_RIOT_SERVICE_TOKEN`과 Riot service의 `MONKEYBOT_RIOT_INTERNAL_TOKEN`은 같은 secret이며,
Riot service는 constant-time으로 비교한다. Internal response는 Axum client가 검증하는 provider-neutral
DTO 자체이고 MonkeyBot public envelope가 아니다. Axum만 이를 `{ data, meta }` public envelope로 감싼다.

Internal error는 `{ "error": { "code", "message" } }`이며 400 invalid value, 401 internal auth,
404 Riot account, 422 invalid JSON shape, 429 provider rate limit, 502 malformed/rejected provider response,
503 timeout/unavailable로 정규화한다. 429의 provider `Retry-After` 초는 internal response header로
보존하고 Axum은 public error의 `details.retryAfterSeconds`로 변환한다. Error body나 log에 internal token과
Riot key를 포함하지 않는다.

Riot integration service는 Riot developer key와 provider HTTP/rate-limit/DTO 변환만 소유한다. Discord
session, CSRF, user/guild authorization, account linking transaction과 PostgreSQL records cache는 Axum
책임이다. Internal route는 nginx/public network에 노출하지 않는다.

external match expand는 Riot 원본 데이터의 KDA/CS/gold/damage/vision/item을
표시할 수 있지만 guild custom match schema와는 별개다.

현재 게임/Spectator endpoint는 만들지 않는다.

## 사용자와 내 정보

### GET /api/users/{discordId}/profile

로그인 없이 읽는 public profile route다. MVP는 현재 UI가 사용하는 `displayName`, Discord ID, 활동
guild 수, season 수만 반환한다. `public_profile_enabled=false`인 사용자와 존재하지 않는 사용자는 같은
`404 not_found`로 숨기며, Riot ID/PUUID, avatar, guild 이름, membership role, MMR history, auction card,
screening은 반환하지 않는다.

```json
{
	"discordId": "1525847677542207548",
	"displayName": "Longcat",
	"activeGuildCount": 2,
	"seasonCount": 3
}
```

`activeGuildCount`는 ACTIVE 사용자 membership과 ACTIVE guild만 센다. `seasonCount`는 그 활동 guild에
속한 ACTIVE/CLOSED season의 distinct 수다. path가 canonical Discord snowflake가 아니면
`400 invalid_discord_id`를 반환한다.

### GET /api/me/champion-preferences

인증된 global user의 전체 snapshot을 반환한다. 값이 없는 lane도 빈 배열로 포함하며 key는 항상 canonical
lane 순서다.

```json
{
	"preferences": {
		"TOP": ["Aatrox", "Gnar"],
		"JUNGLE": ["LeeSin"],
		"MID": [],
		"BOTTOM": ["Ezreal"],
		"SUPPORT": ["Thresh"]
	}
}
```

### PUT /api/me/champion-preferences/{lane}

`lane`은 `TOP|JUNGLE|MID|BOTTOM|SUPPORT`만 허용한다. Body는 해당 lane을 완전히 교체할 ordered
champion ID 목록이다. 빈 배열은 lane 초기화이며 ID는 중복 없이 최대 10개다.

```json
{
	"championIds": ["Ahri", "Orianna", "Syndra"]
}
```

Session과 CSRF를 검증하고 lane 전체 replace transaction을 수행한다. 성공 응답은 수정 lane 하나가 아니라
GET과 같은 다섯 lane의 authoritative `{ preferences: ... }` snapshot이다. Frontend는 요청한 배열을 저장
성공으로 추정하지 않고 응답 snapshot으로 saved/draft state를 교체한다.

### GET /api/me/riot-accounts

### POST /api/me/riot-accounts

```json
{
	"gameName": "Longcat",
	"tagLine": "KR1",
	"platform": "KR"
}
```

Client 입력은 위 세 필드뿐이다. PUUID, regional routing, canonical Riot ID, profile icon은 backend가
Riot integration으로 확인한다. Client가 보낸 PUUID나 profile field를 저장하는 계약은 없다. 외부 조회는
DB transaction 밖에서 끝내고, resolved account를 user에 연결하는 짧은 transaction에서 duplicate와
main cardinality를 검증한다.

### DELETE /api/me/riot-accounts/{puuid}

### PUT /api/me/riot-accounts/{puuid}/main

### POST /api/me/riot-accounts/{puuid}/refresh

모든 self-service mutation은 mutation된 단일 row가 아니라 최신
`{ "riotAccounts": [...] }` snapshot을 success envelope로 반환한다. 따라서 account가 남아 있으면
main account가 정확히 하나라는 server invariant를 frontend가 별도 추론하거나 mutation 직후 다시 GET할
필요가 없다. 저장된 account를 조작하는 child route에서만 opaque PUUID를 path key로 사용한다.

### GET /api/me/privacy

### PUT /api/me/privacy

```text
publicProfile
showRiotIds
allowExternalRecordSearch
```

- `showRiotIds`: 공개 가능한 Discord/길드 사용자 projection에서 그 사용자에게 연결된 Riot ID 관계를
  표시할지 결정한다. Riot ID 자체의 public Riot 전적 검색을 막지 않는다.
- `allowExternalRecordSearch`: 사용자가 속하지 않은 외부인이 공개 guild에서 그 사용자의 guild custom
  match 전적에 접근할 수 있는지 결정한다. 같은 guild 구성원의 원장 조회와 public Riot 전적 검색에는
  적용하지 않는다.

## Guild admin

모든 route는 현재 guild의 ACTIVE member인 OPERATOR를 backend에서 다시 검증한다. Browser mutation은
session cookie와 CSRF header를 모두 검증하고 해당 snapshot의 `expectedRevision`을 비교한다. Page
guard나 client가 보낸 role은 권한 근거가 아니다.

### GET /api/guilds/{guildId}/members

```text
revision       guild_admin_state[MEMBERS].revision
updatedAt      RFC 3339 timestamp
updatedBy      opaque UserId | null
members[]
  id            opaque UserId (Discord snowflake | dmy_<UUIDv7>)
  identityKind  DISCORD | DUMMY
  discordName   guild display name
  riotId        main Riot ID의 GameName#TagLine | null
  role          OPERATOR | MEMBER
  mainLane      TOP | JUNGLE | MID | BOTTOM | SUPPORT | null
  laneMmr       TOP/JUNGLE/MID/BOTTOM/SUPPORT finite number 전체
  registered    boolean
  status        ACTIVE | INACTIVE
  lastActiveAt  RFC 3339 timestamp | null
```

`registered`는 `registered_at IS NOT NULL` projection이고 `role`은 저장된 boolean이 아니라 현재 guild의
operator role과 동기화된 Discord role membership으로 계산한다. DUMMY identity는 Discord role row를
가질 수 없으므로 항상 MEMBER다. `mainLane`과 `lastActiveAt`은 실제 값이 없으면 null이며 임의 lane이나
상대 시간 문자열을 합성하지 않는다. 현재 ACTIVE season에 아직 생성되지 않은 lane MMR은 1500으로
projection한다. Member 정렬은 ACTIVE 우선, `lower(displayName)`, opaque UserId 순서로 고정하며 lane
object는 항상 canonical 다섯 key를 모두 포함한다. ACTIVE season 자체가 없는
setup 미완료 guild의 GET도 다섯 lane을 1500으로 표시하지만 MMR mutation은 409로 거부한다.

아직 MEMBERS state row가 없는 guild의 GET은 row를 생성하지 않고 revision 0, `guild_config`의
updated metadata를 반환한다. 첫 member mutation이 같은 transaction에서 MEMBERS row를 lazy insert하고
lock한다.

이 route는 public profile이 아니라 OPERATOR 전용 관리 projection이다. 따라서 운영자가 연결 상태를
확인할 수 있도록 `showRiotIds`와 무관하게 main Riot ID를 반환하고, `riotId=null`은 실제 미연결만
의미한다. PUUID, routing region, privacy setting 자체는 반환하지 않는다. 공개 profile과 guild records의
Discord-Riot 연결 관계는 계속 `showRiotIds`를 적용한다.

### PATCH /api/guilds/{guildId}/members/{memberId}/mmr

```text
lane
delta
note
expectedRevision
```

summary MMR 직접 덮어쓰기가 아니라 MANUAL_ADJUSTMENT ledger를 남긴다.
CSRF와 browser `Idempotency-Key`가 필수다. Header는 중복 없이 visible ASCII 1..128 bytes를 허용하고
공식 browser client는 UUID를 `crypto.randomUUID()`로 생성한다. Bot route의 Discord interaction
snowflake key와는 별도 adapter 계약이다. 같은 key와 같은 payload 재전송은 저장된 응답을 replay해
ledger를 중복 append하지 않고, 같은 key의 다른 payload는 409를 반환한다.

`delta`는 finite non-zero number이고 `note`는 trim 후 1..500자다. 활성 season이 없으면
`guild_setup_required` 409를 반환한다. 성공 응답은 변경 row만이 아니라 GET과 동일한 최신 full member
snapshot을 반환한다.

### PATCH /api/guilds/{guildId}/members/{memberId}/status

```text
status             ACTIVE | INACTIVE
expectedRevision   guild_admin_state[MEMBERS].revision
```

status mutation은 CSRF와 revision으로 직렬화하며 별도 idempotency key를 요구하지 않는다. INACTIVE 전이는
`inactivated_at`을 기록하고 ACTIVE 복구는 이를 비운다. `registered_at`, 과거 MMR과 ledger는 유지한다.
성공 응답은 GET과 동일한 최신 full member snapshot이다.

인증된 actor와 `memberId`가 같고 요청 상태가 `INACTIVE`이면 현재 유일한 운영자 수와 무관하게
`409 self_deactivation_forbidden`을 반환한다. 자기 자신을 비활성화해 admin 접근 권한을 잃는 흐름을
허용하지 않기 위한 command invariant다. 별도 guild OWNER 모델은 두지 않으며, 다른 member의 상태 변경은
기존 OPERATOR 권한과 revision 규칙을 그대로 따른다.

두 mutation은 MEMBERS row를 lock하고 expected revision을 비교한 뒤 domain write, audit log, MEMBERS
revision/updated metadata 갱신을 하나의 transaction으로 commit한다. MMR은 OPERATOR 권한 확인 후
idempotency acquire/replay를 revision lock보다 먼저 수행한다. Audit action/target은 MMR에서
`MEMBER_MMR_ADJUSTED`/`GUILD_MEMBER_MMR`, status에서
`MEMBER_STATUS_CHANGED`/`GUILD_MEMBER`다. 409이면 domain/audit/revision을 변경하지 않는다. Admin
members 화면은 WebSocket topic이나 invalidate outbox를 사용하지 않으며 client는 성공 snapshot으로
교체하고 409에서 GET으로 최신 snapshot을 다시 읽는다.

### GET /api/guilds/{guildId}/settings

```text
revision
updatedAt
updatedBy | null
catalogUpdatedAt | null
settings
  displayName
  operatorRoleId
  recruitmentChannelId | null
  entryChannelId | null
  resultChannelId | null
  logChannelId | null
  isExternallyPublic
roles[]
  id
  name
channels[]
  id
  name
  parentName | null
```

ACTIVE guild의 ACTIVE OPERATOR 전용 snapshot이다. 설정과 catalog는 `REPEATABLE READ` transaction에서
읽는다. `roles`는 managed role을 제외하고 Discord position 내림차순, 이름, ID로 정렬한다. `channels`는
Bot이 동기화한 게시 가능한 channel을 position, 이름, ID로 정렬한다. 첫 catalog sync 전에는
`catalogUpdatedAt=null`이고 두 목록은 비어 있을 수 있다. 현재 설정 ID가 이후 Discord에서 삭제되어
목록에 없더라도 `settings` 값은 보존하며 frontend는 이를 누락된 현재 설정으로 표시한다.

### PUT /api/guilds/{guildId}/settings

```text
displayName            trim 후 1..100자
operatorRoleId         Discord role snowflake
recruitmentChannelId   required Discord channel snowflake
entryChannelId         Discord channel snowflake | null
resultChannelId        Discord channel snowflake | null
logChannelId           Discord channel snowflake | null
isExternallyPublic     boolean
expectedRevision       guild_config.revision, 0 이상
```

Session cookie, CSRF cookie/header와 Origin 검증이 필요하며 request는 위 여덟 field만 허용한다. 현재
`guild_config` row를 lock하고 변경 전 `operatorRoleId`와 동기화된 member role로 actor를 검증한 뒤
revision을 비교한다. 변경된 운영자 role은 non-managed catalog role이어야 하고, 변경된 non-null channel은
catalog에 있어야 한다. Catalog 교체로 사라진 기존 ID를 그대로 유지하는 rename/public-only update는
허용하지만 새 누락 ID 선택은 `422 invalid_settings_selection`으로 거부한다. `resultChannelId=null`이면
결과 notification을 생략한다. `entryChannelId`는 fallback으로 사용하지 않는다.
`recruitmentChannelId=null`은 `422 recruitment_channel_required`와 `field=recruitmentChannelId`로 거부한다.
GET에는 migration 이전 legacy `recruitmentChannelId=null`이 남아 있을 수 있으며, 운영자는 유효한 채널을
선택해 저장하여 복구한다.

성공 transaction은 `guild_config.revision`을 1 올리고 `updated_by/updated_at`과
`GUILD_SETTINGS_UPDATED`/`GUILD_CONFIG` audit before/after summary를 기록한다. 응답은 GET과 동일한 최신
full snapshot이다. stale revision은 `409 revision_conflict`와 현재 revision/update metadata를 반환하고
DB를 변경하지 않는다. 별도 idempotency key나 WebSocket은 사용하지 않는다.

시즌 field는 strict payload에서 거부한다. 시즌 조회/전환은 settings aggregate와 분리한 아래 seasons
query/command를 사용한다.

### POST /api/guilds/{guildId}/data-export

현재 guild의 ACTIVE OPERATOR browser session과 CSRF cookie/header, Origin 검증이 필요하다. Body와
`Idempotency-Key`는 받지 않는다. Export는 실행 시점의 plain JSON snapshot을 만들고
`DATA_TRANSFER_EXPORTED`/`MEMBER_SEASON_MMR_SEED` audit을 남기는 read command이므로 같은 요청을 replay하지
않는다.

성공 response:

```text
data
  archive
    format                 MONKEYBOT_DATA_TRANSFER
    kind                   MEMBER_SEASON_MMR_SEED
    schemaVersion          1
    exportedAt             RFC 3339 timestamp
    source
      guildId
      guildName
      season
        id                 UUID
        name
    members[]              최대 1,000 member
      discordId
      displayName
      lanes[]              정확히 canonical 5개, { lane, mmr }
  fileName
meta
```

현재 ACTIVE season이 없으면 `409 no_active_season`이다. Export는 `REPEATABLE READ` snapshot에서 ACTIVE
Discord member만 읽고, 아직 materialize되지 않은 lane도 1500으로 projection해 다섯 lane을 모두 낸다.
`identity_kind=DUMMY`와 INACTIVE member는 제외한다. `gamesPlayed`, `wins`, Riot account/PUUID, privacy,
champion preference, Discord role/channel/config, dummy profile, 모집/시리즈/경매/픽밴/game/ledger history,
audit/outbox/idempotency row는 archive에 넣지 않는다.

활성 Discord member가 1,000명을 초과하면 export는 `422 archive_member_limit_exceeded`로 거절하며 partial
archive를 만들지 않는다.

Web은 archive object를 indent 2의 readable JSON file로 내려받는다. archive는 암호화하거나 서명하지 않으며,
운영자는 MMR 값과 source metadata를 검토하거나 편집한 뒤 preview에 올릴 수 있다. `mookbot_v3` D1/Valkey
dump나 raw database export는 이 endpoint의 output이 아니며 이 API로 import할 수 없다.

### POST /api/guilds/{guildId}/data-imports/preview

현재 guild의 ACTIVE OPERATOR browser session과 CSRF cookie/header, Origin 검증이 필요하다.

```text
archive                 data-export response의 archive object
```

Request의 정상 JSON object는 위 field 하나만 받는다. Content-Type 누락, malformed JSON, outer object의
unknown field는 공통 request normalizer의 `unsupported_media_type`/`bad_request`/`unprocessable_entity`로
반환한다. 정상 outer object 안의 archive에 대해서 Server는 `MONKEYBOT_DATA_TRANSFER`, kind
`MEMBER_SEASON_MMR_SEED`, schema version `1`, canonical five lane uniqueness/finite MMR와 payload size를
검증한다. Archive format/shape 오류는 `400 invalid_archive`로 반환한다. Preview는 MMR, guild member, game,
ledger를 변경하지 않는다.

성공 response:

```text
data
  importId               UUID
  digest                 archive의 SHA-256 hex digest
  expiresAt              RFC 3339 timestamp, preview 생성 후 15분
  source
    guildId
    guildName
    season
      id                  UUID
      name
  summary
    archiveMembers
    matchedMembers
    laneEntries
    changes
  conflicts[]
    reason               member_not_active
    message              사용자에게 안전하게 표시할 수 있는 구체적 이유
    discordId
    displayName
  targetRevision         guild_admin_state[MEMBERS].revision
meta
```

Archive source guild/season은 file-declared provenance metadata이며 authenticated origin이 아니다. 같은 guild만
강제하지 않지만 archive의 모든 Discord ID가 target guild에 이미 존재하는 ACTIVE Discord member여야 한다.
Import는 `users`, `guild_members`, Riot account,
privacy, Discord role/channel 또는 dummy identity를 만들거나 복구하지 않는다. `conflicts`가 하나라도 있으면
preview는 검토 결과로만 남고 apply할 수 없다.

중복 Discord ID/lane, canonical five lane 불충족, finite 하지 않은 MMR 같은 archive 구조 오류는 preview
conflict가 아니라 `400 invalid_archive`다. archive와 대상 MMR의 차가 finite ledger delta로 계산되지 않으면
`422 invalid_mmr_transfer`로 거절한다. 검증을 통과한 archive payload와 digest, actor ID, target guild,
target MEMBERS revision, target MMR snapshot digest, expiry, summary와 conflict count는 짧게 보존되는
`guild_data_imports` preview row에 함께 기록한다. Full conflict list는 response에만 둔다. Preview 생성은
`DATA_IMPORT_PREVIEWED`/`DATA_IMPORT` audit을 남긴다. Browser가 검토 뒤 변경한 archive object를 apply body에
재전송하는 방식은 허용하지 않는다. Apply 성공 시 archive payload는 즉시 제거하며 API process의 60초 retention
task가 만료된 `PREVIEWED` row를 삭제한다.

### POST /api/guilds/{guildId}/data-imports/{importId}/apply

현재 guild의 ACTIVE OPERATOR browser session, CSRF cookie/header, Origin 검증과 browser
`Idempotency-Key`가 필요하다.

```text
expectedDigest           preview response의 digest
expectedTargetRevision   preview response의 targetRevision
```

Server는 import row가 존재하고 같은 actor/guild에 속하는지, 만료되지 않았는지, digest와 MEMBERS revision,
preview 당시 target lane MMR snapshot이 일치하는지, conflict가 없는지, 현재 ACTIVE season이 존재하는지와
active workflow가 없는지를 transaction 안에서 다시 검증한다. `preview_not_found`, `preview_expired`,
`preview_actor_mismatch`, `preview_digest_mismatch`, `preview_conflicts`, `revision_conflict`,
`mmr_state_changed`, `active_workflow_conflict`, `invalid_mmr_transfer`는 각각 안전한 원인과 request ID를 포함해 반환하며
MMR/audit/revision을 변경하지 않는다.

Apply body의 malformed JSON, unknown field, type mismatch도 공통 request normalizer의 `bad_request` 또는
`unprocessable_entity`를 반환한다. 정상 JSON body의 음수 revision 또는 digest 형식 오류는
`422 invalid_import_apply_command`다.

Apply는 member/season/lane MMR row를 Discord ID와 canonical lane 순으로 확보하고 lock한다. 실제 값이
달라지는 lane마다 `DATA_IMPORT_SEED` ledger를 append하고 summary MMR을 같은 transaction에서 갱신한다.
각 entry의 game/win delta는 0이며 기존 `games_played`, `wins`, game history와 series/auction history는
그대로 유지한다. Import는 partial success가 없고, conflict가 있는 일부 member만 건너뛰어 적용하지 않는다.

성공 response:

```text
data
  importId
  status                 APPLIED
  appliedAt              RFC 3339 timestamp
  summary                preview와 같은 shape
  targetRevision         적용 후 guild_admin_state[MEMBERS].revision
meta
```

Idempotency record, changed summary/ledger, `DATA_IMPORT_APPLIED`/`DATA_IMPORT` audit, MEMBERS revision과 import
status는 하나의 transaction으로 commit한다. 이 operation은 generic checkpoint/revert나 database restore를
만들지 않으며, 완료된 import의 archive를 다시 apply하지 않는다.

### POST /api/guilds/{guildId}/full-data-export

ACTIVE OPERATOR browser session, CSRF cookie/header와 Origin 검증이 필요하다. `REPEATABLE READ` snapshot에서
`GUILD_FULL_SNAPSHOT` V1 archive를 만들며 body와 idempotency key는 받지 않는다. Export에는 global 사용자
identity/Riot/privacy/선호 챔피언, guild member, 시즌, 일반 모집/시리즈, 경매 전체 원장, 픽밴/게임/통계,
현재 MMR/ledger와 관리자 audit이 포함된다. Session/OAuth secret, Discord role/channel catalog·guild config,
cache/outbox/idempotency row는 제외한다.

```text
data
  archive              MONKEYBOT_DATA_TRANSFER / GUILD_FULL_SNAPSHOT / schemaVersion 1
    source             { guildId, guildName }
    tables             strict 41-table row arrays, total 최대 250,000 rows
  fileName
  summary
    totalRows, users, members, seasons, recruitments, series, games
    auctionTournaments, auctionMatches, mmrLedgerEntries, auditEntries
meta
```

성공 시 `FULL_DATA_TRANSFER_EXPORTED` audit을 남긴다. Request body 기본 한도는 전체 archive 계약과 동일한
16 MiB다.

### POST /api/guilds/{guildId}/full-data-imports/preview

Body는 `{ archive }` 하나만 받는다. Archive의 source guild ID가 path guild와 다르면 모든 정확한 source
guild 참조를 path target ID로 매핑하고 `remapsSourceGuild=true`로 명시한다. 이때 source guild의 모집글과
시리즈/경매 결과 카드 `channel_id`/`message_id`는 `null`로 정리한다. Target의
현재 active season에 모집·시리즈·경매·game/MMR domain row가 있으면
`target_active_season_not_empty` conflict를 반환한다. Closed season 이력은 importability를 막지 않는다.
현재 active season이 비어 있으면 archive active season 참조를 그 target season ID로 매핑하며 response의
`usesTargetActiveSeason=true`로 명시한다. 모집·시리즈·경매 display number는 기존 target history의 namespace별
최대 번호와 counter 뒤로 이동하고 `rebasesDisplayNumbers=true`로 명시한다.

Global 사용자 정보는 기존 MonkeyBot 값을 우선한다. 동일 사용자에게 Riot account가 하나라도 있으면 archive의
그 사용자 Riot account 묶음을 insert하지 않는다. `user_champion_preferences`도 동일 사용자·lane에 현재 row가
하나라도 있으면 archive의 해당 lane 묶음을 insert하지 않는다. 현재 값이 없는 사용자/line만 archive 값으로 보충한다.
이 병합은 과거 경기·MMR domain row import 범위를 줄이지 않는다.

Importable target에서는 archive active season ID를 target active season ID로 재작성하고, 해당 source season
row는 제외한 뒤 transaction SAVEPOINT에서 41 table을 실제 import order로 insert해 PostgreSQL
FK/CHECK/UNIQUE 제약을 확인한다. Closed season은 재작성하지 않는다. 검증 성공/실패와 무관하게 savepoint까지
rollback하므로 preview가 domain row를 바꾸지 않는다. 성공 response는 `importId`, archive `digest`,
`targetDigest`, 15분 `expiresAt`, source, summary, conflicts, `remapsSourceGuild`와
`usesTargetActiveSeason`, `rebasesDisplayNumbers`를 반환한다. 원본 source metadata/digest는 바꾸지 않는다. Preview row/audit만
savepoint 밖 transaction에서 commit한다.

### POST /api/guilds/{guildId}/full-data-imports/{importId}/apply

같은 preview actor의 ACTIVE OPERATOR session, CSRF/Origin과 `Idempotency-Key`가 필요하다.

```text
expectedDigest          preview response의 digest
expectedTargetDigest    preview response의 targetDigest
```

Server는 actor/status/expiry/digest/conflicts와 저장된 source guild를 확인하고 guild lock 아래에서 target
digest와 importability를 다시 계산한다. 하나라도 달라지면 적용하지 않고 구체적인 stable error와 request ID를
반환한다. 성공 시 preview와 동일한 target guild/active season ID 매핑을 적용한 모든 archive row, import status,
payload 제거,
`FULL_DATA_IMPORT_APPLIED` audit과 idempotency response를 한 transaction으로 commit한다. Response의
`remappedSourceGuild`, `usedTargetActiveSeason`, `rebasedDisplayNumbers`가 실제 매핑 여부를 나타낸다. Partial apply와 완료 archive
payload 재사용은 지원하지 않는다.

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

ACTIVE guild의 ACTIVE OPERATOR session만 조회할 수 있다. Query는 다음 두 field만 허용한다.

```text
cursor   이전 응답의 opaque nextCursor | 생략
limit    1..100, 기본 50
```

응답은 append-only `admin_audit_log`의 canonical field와 현재 표시명 projection을 함께 반환한다.

```text
entries[]
  id                       uuid
  principalType            USER | BOT | SYSTEM | MIGRATION
  principalId              canonical principal identifier
  principalDisplayName     현재 display name | null
  action                   canonical action code
  targetType               canonical target type | null
  targetId                 canonical target identifier | null
  targetDisplayName        현재 display name | null
  checkpointId             uuid | null
  requestId                string | null
  beforeSummary            JSON | null
  afterSummary             JSON | null
  result                   APPLIED | REVERTED
  note                     string | null
  createdAt                RFC 3339 timestamp
nextCursor                 opaque string | null
```

정렬은 `(created_at DESC, id DESC)`이고 cursor 이후 조건은 같은 tuple의 strict keyset 비교다. 구현은
`limit + 1`개를 읽어 다음 페이지 존재 여부를 판단하며 client는 cursor 내용을 해석하거나 만들지 않는다.
잘못된 cursor는 `400 invalid_cursor`, 범위를 벗어난 limit은 `400 invalid_limit`이다.

`principalDisplayName`은 USER principal의 현재 guild member 표시명을 우선하고 현재 guild member
projection이 없으면 global user 표시명을 사용한다. USER가 아닌 principal은 null이다.
`targetDisplayName`은 `GUILD_MEMBER`/`GUILD_MEMBER_MMR`이면 현재 guild member 표시명,
`GUILD`/`GUILD_CONFIG`이면 현재 guild 설정 표시명, `SEASON`이면 현재 저장된 `seasons.name`,
`RECRUITMENT`이면 현재 저장된 `recruitments.title`이고 나머지는 null이다. 감사 row에는 표시명의 역사적
snapshot이 없으므로 두 표시명은 이벤트 발생 당시 이름을 보장하지 않는다. 표시명 projection이 없어도 row를
제외하지 않는다.

`beforeSummary`/`afterSummary`는 저장된 JSON을 변형하지 않고 반환한다. Category, 한국어 action label과
상세 문장은 저장 column이 아니므로 Web presentation adapter가 알려진 action code를 변환하고, 모르는
action은 raw code와 `기타` category로 표시한다. 첫 페이지 이후 Web은 `더 보기`로 다음 cursor를 조회해
중복 ID 없이 누적하고, 검색/category/실행자 필터는 현재까지 누적한 row에 local로 적용한다.

웹과 Bot이 같은 관리 작업을 제공하면 인증 adapter 아래의 같은 application command와 audit
transaction을 사용한다. 일괄 등록, 시즌 초기화와 오래된 내전 정리는 MVP 웹에 노출하지 않는다.

### GET /api/guilds/{guildId}/seasons

ACTIVE guild의 ACTIVE OPERATOR session만 조회할 수 있다.

```text
current
  id
  name
  status                 ACTIVE
  startedAt
  endedAt                null
  createdBy              member display name | null
  participantCount
  generalSeriesCount
  auctionTournamentCount
  confirmedGameCount
  auctionTenCount
  auctionTwentyCount
  lastGameAt             timestamptz | null
completed[]              같은 shape, status=CLOSED
```

`current`는 현재 ACTIVE season이며 `completed`는 종료 시각 최신순의 CLOSED season이다. ACTIVE season이
없으면 `409 no_active_season`이다. `participantCount`는 CONFIRMED game의 distinct 참가자,
`generalSeriesCount`는 삭제되지 않은 COMPLETED 일반 시리즈, auction count 세 개는 삭제되지 않은
COMPLETED 경매 내전, `confirmedGameCount`와 `lastGameAt`은 CONFIRMED game에서 집계한다. Display 날짜나
`durationLabel`은 반환하지 않으며 Web이 RFC 3339 timestamp를 사용자 locale에 맞게 표현한다.

### POST /api/guilds/{guildId}/seasons

Web 운영자가 현재 시즌을 종료하고 새 ACTIVE season을 시작하는 command다. Session cookie, CSRF
cookie/header와 Origin 검증, Browser용 `Idempotency-Key`가 필요하다.

```text
name                      trim 후 1..100자
expectedCurrentSeasonId   현재 ACTIVE season UUID
```

Request는 위 두 field만 허용한다. 성공 응답은 GET과 같은 최신 `{current, completed}` snapshot이다.
과거 game/MMR/auction row를 삭제하거나 초기화하지 않는다.

```text
Session ACTIVE OPERATOR 권한 검사
-> idempotency 확보 또는 동일 요청 replay
-> current ACTIVE season SELECT FOR UPDATE
-> expectedCurrentSeasonId 확인
-> 기존 season CLOSED + ended_at
-> 새 ACTIVE season insert
-> SEASON_ROTATED/SEASON audit와 idempotency response 저장
-> commit
```

이 작업 전체는 한 transaction이다. 다른 current ID를 보낸 요청은 `409 current_season_conflict`, 같은
key의 다른 요청은 `409 idempotency_conflict`이며 domain/audit를 변경하지 않는다. 같은 key와 body의
재전송은 저장된 성공 응답을 반환한다. 처리 중인 같은 요청은 `409 idempotency_in_progress`다. `seasons`의
guild별 ACTIVE partial unique도 concurrent rotate의 두 번째 commit을 차단한다.

Audit target은 새 ACTIVE season ID이며 before에는 기존 ACTIVE season, after에는 종료된 기존 season과
새 ACTIVE season을 기록한다.

향후 Bot `/새시즌`은 이 public Web endpoint에 Bot credential을 섞지 않는다. 별도 Bot service-principal
adapter가 Discord interaction ID를 idempotency key로 사용하고 동일 season rotate application command를
호출한다.

## Service admin

### GET /api/admin/status

Session과 service-admin allowlist가 모두 필요하다. 미인증은 `401`, 로그인했지만 allowlist에 없는 사용자는
`403 service_admin_required`다.

```text
API
PostgreSQL
Bot heartbeat
Valkey
```

성공 body는 `{ checkedAt, services }`이고 service key는 `API`, `POSTGRESQL`, `BOT_HEARTBEAT`, `VALKEY`다.
각 service는 `status`(`normal|delayed|down`), `latencyMs|null`, 사람이 읽는 `value`,
`lastHealthyAt|null`, `issue|null`을 가진다. Data Dragon과 internal notify를 별도 tile로 만들지 않는다.

Bot heartbeat는 newest persisted `bot_instances.last_heartbeat_at`을 기준으로 `<=90초 normal`,
`>90초~<=5분 delayed`, 미수신 또는 `>5분 down`이다. stale Bot도 마지막 persisted timestamp는
`lastHealthyAt`으로 유지한다. API가 PostgreSQL row를 읽을 수 없으면 status aggregate 자체를 만들 수 없으므로
`503 database_unavailable`을 반환한다.

### GET|POST /api/admin/guilds/{guildId}/record-reset

개발/테스트 환경의 전체 기록 초기화 도구다. 두 method 모두 로그인한 service admin allowlist만 허용하고
guild operator 여부는 검사하지 않는다. 또한 `MONKEYBOT_DUMMY_USERS_ENABLED=true`가 필요하며 이 설정은
production startup에서 거부된다.

POST는 runtime role에 append-only 원장의 직접 `DELETE` 권한을 부여하지 않는다. migration이 만든
guild-scoped `service_admin_reset_guild_records(text)` security-definer 함수가 FK 순서대로 모든 이력을
제거하고, API가 빈 active season을 재생성하는 작업까지 같은 transaction에서 처리한다. 함수 실행 권한도
개발 DB의 runtime grant에서 `service_admin_reset_enabled=true`를 명시한 경우에만 부여한다.

GET은 대상 길드 이름, 유지할 ACTIVE 멤버 수, 현재 시즌 이름, 삭제 대상 요약과 정확한 확인문구
`RESET {guildId}`를 반환한다. POST는 CSRF, UUID `Idempotency-Key`, GET이 준 확인문구를 요구한다. 확인문구가
다르면 `422 reset_confirmation_mismatch`다.

POST transaction은 시즌·일반/경매 모집·시리즈·게임·MMR·import 작업·workflow audit/outbox/idempotency를
삭제하고, 기존 ACTIVE 시즌 이름을 가진 빈 ACTIVE 시즌 하나와 네 display counter(`next_value=1`)를 다시
만든다. `guilds`, `guild_members`, `guild_config`, Discord role/channel catalog, Riot 계정과 사용자 설정은
보존한다. 같은 key와 같은 길드의 재전송은 저장된 성공 응답을 반환하며 다른 길드에 key를 재사용하면
`409 idempotency_conflict`다.

### GET /healthz

process liveness only.

### GET /readyz

PostgreSQL과 migration version 필수. Valkey 장애는 degraded로 보고한다.

### GET /metrics

VPS internal network only.

## Internal Bot API

internal route는 user cookie가 아니라 service principal을 사용한다.

### POST /internal/heartbeat

BotBearer 전용의 status 갱신 route다.

```json
{
	"instanceId": "bot-main-1",
	"version": "0.1.0",
	"commitSha": "abc1234"
}
```

`instanceId`는 `[A-Za-z0-9][A-Za-z0-9._:-]*` 1..128 bytes, `version`은 visible ASCII 1..100 bytes,
`commitSha`는 nullable `[A-Za-z0-9._-]{1,128}`이다. backend는 instance row를 upsert하되 최초
`started_at`은 유지하고 version/commit/last heartbeat만 갱신한다. 성공 응답은
`{ "accepted": true, "lastHeartbeatAt": "RFC 3339" }`이며 invalid body는 `422 invalid_heartbeat`다.

### POST /internal/notifications/claim

Bot service principal만 사용한다. request는 `{ "instanceId": "bot-1", "limit": 20 }`이고 `limit`은
1..50이다. Worker가 available 또는 expired-lease row를 claim하며 response notification은
`id`, `guildId`, `leaseToken`, `leaseUntil`, `channelId`, `type`, `payload`, `discordNonce`, `attempts`를
포함한다. lease는 30초이고 하나의 row는 최대 8회 claim한다. `discordNonce`는 notification ID에서
유도한 stable 22-character base64url 값이므로 create 재시도 중 Discord 중복을 구분할 수 있다.

### POST /internal/notifications/ack

request는 `{ "instanceId", "notificationId", "leaseToken", "discordMessageId" }`다. owner+token이
현재 lease와 일치할 때만 generic notification을 delivered로 전이한다. `RECRUITMENT_MESSAGE_UPDATE`와
`RECRUITMENT_MESSAGE_DELETE`, `AUCTION_RECRUITMENT_MESSAGE_UPDATE`,
`AUCTION_RECRUITMENT_MESSAGE_DELETE`, `ENTRY_CONFIRMATION_CREATE`,
`AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE`는 Discord delivery 성공 뒤 이 route를 사용한다.
`RECRUITMENT_MESSAGE_CREATE` 또는 `AUCTION_RECRUITMENT_MESSAGE_CREATE`에는 이 route를 사용하지 않으며
호출하면 typed ack required safe reason으로 거절한다.

### POST /internal/notifications/recruitment-message/ack

`RECRUITMENT_MESSAGE_CREATE` 전용 acknowledgement다. request shape은 generic ack와 같고 Bot은 Discord
채널 send가 성공한 뒤 message ID를 보낸다. Backend는 notification delivered 전이와
`recruitments.channel_id/message_id` bind를 하나의 transaction으로 확정한다. bind 시 create payload보다
현재 revision이 앞서 있으면 UPDATE를, 현재 status가 CANCELLED면 DELETE를 같은 transaction에 후속
적재한다. generic ack 뒤 별도 bind를 호출하는 흐름은 crash window와 duplicate message 위험 때문에
금지한다.

### POST /internal/notifications/auction-recruitment-message/ack

`AUCTION_RECRUITMENT_MESSAGE_CREATE` 전용 acknowledgement다. request shape은 generic ack와 같지만
다른 aggregate를 bind하므로 일반 모집 acknowledgement와 polymorphic body를 공유하지 않는다. Bot은 Web이
생성한 경매 모집 message send 성공 뒤 message ID를 보낸다. Backend는 notification delivered 전이와
`auction_recruitments.channel_id/message_id` bind를 하나의 transaction으로 확정한다. generic ack 뒤 별도
bind를 호출하는 흐름은 허용하지 않는다.

### POST /internal/notifications/nack

request는 `{ "instanceId", "notificationId", "leaseToken", "failure" }`다. failure는
`HTTP` (`status`, 429일 때 선택 `retryAfterMs`), `TRANSPORT`, `DELIVERY_UNKNOWN` 중 하나다. Backend가
retryable 여부, backoff, MAX_ATTEMPTS, DELIVERY_UNKNOWN terminal 전이를 결정한다. 429/5xx/transport만
retry하며 ordinary 4xx, unknown delivery, 8회 소진은 terminal failure다.

Bot의 domain command는 별도 약식 로직을 만들지 않고 위 guild-scoped
application command를 service principal로 호출한다. Discord interaction ID를
Idempotency-Key로 사용한다.

## WebSocket

### GET /ws

cookie session으로 handshake한다.

현재 subscribe를 허용하는 topic:

```text
guild:{guildId}:recruitment:{id}
guild:{guildId}:recruitment-entry-draft:{id}
guild:{guildId}:series:{id}
guild:{guildId}:auction-recruitment:{id}
guild:{guildId}:auction-tournament:{id}
```

`auction-recruitment` topic은 OPEN 또는 CLOSED 모집의 현재 active operator만 subscribe할 수 있다. Web은
`canManageMembers`가 true인 모집 detail에서만 이 topic을 열고, `invalidate` 또는 `resync_required`를 받으면
authoritative auction recruitment snapshot을 다시 읽는다.

`auction-tournament` topic은 `BIDDING` 상태의 현재 active operator만 subscribe할 수 있다. Web은
`canManageDraft=true`인 입찰 작업대에서만 이 topic을 열고, 더 높은 revision invalidate, reconnect,
`resync_required` 뒤 authoritative tournament snapshot을 다시 읽는다. 입찰 command는 HTTP의 CSRF,
Idempotency-Key, expectedRevision transaction으로만 확정하며 WebSocket은 mutation payload를 받지 않는다.
일반 guild member, `BRACKET_SETUP` 이후 tournament, `auction-match`는 socket을 열지 않는다. auction match
topic은 namespace 예약 상태이며 endpoint가 subscribe하지 않는다.

client는 `{ "type": "subscribe", "topics": [...] }` 또는 `unsubscribe` frame을 보낸다. 성공하면
`{ "type": "subscribed", "topics": [...] }`, hub lag에는 `{ "type": "resync_required" }`, 거부에는
`{ "type": "error", "code", "message" }`를 보낸다. Origin/session 검사는 HTTP upgrade 전에 수행한다.

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
- recruitment topic event의 revision은 recruitment aggregate만, entry-draft topic event의 revision은
  `recruitment_entry_drafts` aggregate만 나타낸다. 두 topic은 동일 recruitment ID를 써도 revision을
  교환하거나 비교하지 않는다.
- auction-recruitment topic event의 revision은 `auction_recruitments` aggregate만 나타낸다. setup draft,
  auction tournament, auction match revision과 교환하거나 비교하지 않는다.
- auction-tournament topic event의 revision은 `auction_tournaments.revision`만 나타낸다. 모집, bracket,
  match revision과 교환하거나 비교하지 않는다.

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
