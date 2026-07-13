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
		"globalDisplayName": "Longcat",
		"displayName": "MSM Longcat",
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
판단하지 않는다. 응답에는 Discord user ID를 독립 field로 넣지 않으며 PUUID, routing region과 계정 소유
내부 key도 반환하지 않는다. `avatarUrl`은 Discord CDN provider URL이므로 URL path에는 provider가 요구하는
Discord ID가 포함될 수 있다.

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
guild의 내부 game/MMR/champion 통계는 개인 privacy 설정으로 숨기지 않는다. Discord user ID는 독립 field로
반환하지 않지만 profile과 같은 Discord CDN `avatarUrl` path에는 provider ID가 포함될 수 있다. PUUID,
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

통합/라인 탭, player 검색, 표시 인원 제한, 순위와 승률은 frontend가 받은 snapshot에서 계산한다. 이
endpoint는 pagination/filter query와 WS를 사용하지 않는다.

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

MVP는 현재 UI가 사용하는 displayName, Discord ID, 활동 guild 수, season
수부터 제공한다. MMR history, auction card, screening은 UI가 생기기 전
선행 구현하지 않는다.

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
recruitmentChannelId   Discord channel snowflake | null
entryChannelId         Discord channel snowflake | null
logChannelId           Discord channel snowflake | null
isExternallyPublic     boolean
expectedRevision       guild_config.revision, 0 이상
```

Session cookie, CSRF cookie/header와 Origin 검증이 필요하며 request는 위 일곱 field만 허용한다. 현재
`guild_config` row를 lock하고 변경 전 `operatorRoleId`와 동기화된 member role로 actor를 검증한 뒤
revision을 비교한다. 변경된 운영자 role은 non-managed catalog role이어야 하고, 변경된 non-null channel은
catalog에 있어야 한다. Catalog 교체로 사라진 기존 ID를 그대로 유지하는 rename/public-only update는
허용하지만 새 누락 ID 선택은 `422 invalid_settings_selection`으로 거부한다.

성공 transaction은 `guild_config.revision`을 1 올리고 `updated_by/updated_at`과
`GUILD_SETTINGS_UPDATED`/`GUILD_CONFIG` audit before/after summary를 기록한다. 응답은 GET과 동일한 최신
full snapshot이다. stale revision은 `409 revision_conflict`와 현재 revision/update metadata를 반환하고
DB를 변경하지 않는다. 별도 idempotency key나 WebSocket은 사용하지 않는다.

시즌 field는 strict payload에서 거부한다. 시즌 조회/전환은 settings aggregate와 분리한 아래 seasons
query/command를 사용한다.

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
