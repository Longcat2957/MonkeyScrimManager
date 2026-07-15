# MonkeyBot PostgreSQL Schema Plan

작성일: 2026-07-12

## 문서 지위

이 문서는 MonkeyBot PostgreSQL schema와 SQLx migration의 기준서다. 단순 관계도가
아니라 실제 migration을 작성할 때 따라야 하는 table, key, constraint, index,
transaction, delete, legacy import 정책을 정의한다.

판단 우선순위는 다음과 같다.

1. 현재 구현된 SvelteKit route, component props, command type, fixture의 의미
2. 이 저장소의 business workflow 문서
3. `../mookbot_v3`의 검증된 MMR과 운영 데이터

충돌 시 frontend를 기준으로 이 문서를 수정한다. 기존
`apps/web/src/lib/server/db/schema.ts`는 SQLite/Drizzle 초기 흔적이므로 PostgreSQL 설계
근거로 사용하지 않는다.

관련 문서:

- [Backend Development Plan](09-backend-development-plan.md)
- [Realtime Collaboration](07-realtime-collaboration.md)
- [Backend Routes](routing/backend.md)

## 확정된 설계

- 영속 DB는 VPS 내부 PostgreSQL 하나다.
- PostgreSQL에는 Axum backend와 별도 migration job만 접근한다.
- SvelteKit은 frontend-only이며 DB에 직접 접근하지 않는다.
- Valkey는 Pub/Sub, presence, rate limit만 담당한다. Riot 전적 snapshot과 recent page cache는
  PostgreSQL에 영속화한다.
- Discord guild가 tenant 경계다. 모든 운영 데이터는 `guild_id`로 격리한다.
- Discord snowflake는 `text`, 신규 domain ID는 application이 생성한 UUIDv7 `uuid`다.
- timestamp는 전부 UTC `timestamptz`다.
- canonical lane은 `TOP | JUNGLE | MID | BOTTOM | SUPPORT`다.
- `best_of` storage는 `1 | 3 | 5`를 허용한다. 일반 내전은 모집 생성 시 BO1/BO3/BO5를 선택하고,
  선택값을 series에 immutable snapshot으로 복사한다. 경매 match는 별도 workflow이며 현재 BO3로 고정한다.
- MMR은 v3의 소수 ELO를 보존하기 위해 `double precision`이다. 반올림은 UI에서만 한다.
- 경매 point와 count는 `integer`, revision과 순번은 `bigint`다.
- 상태값은 초기에는 PostgreSQL enum 대신 `text + CHECK`를 사용한다.
- 핵심 aggregate를 JSONB 한 덩어리로 저장하지 않는다.
- JSONB는 versioned checkpoint, audit 요약, idempotency response, outbox payload와 검증된 외부 Riot
  records DTO snapshot에만 쓴다.
- 일반 내전과 경매 내전의 확정 game은 같은 game/MMR 원장을 사용한다.
- 신규 경매 game도 guild MMR에 반영한다.
- 기존 v3 경매 game에는 MMR을 소급 계산하지 않는다.
- database trigger로 `updated_at`이나 revision을 몰래 변경하지 않는다. application transaction이
  명시적으로 기록한다.
- 초기에는 RLS, table partition, materialized view를 도입하지 않는다.

## 명명과 표기

이 문서의 table 표기는 다음 의미다.

```text
PK       primary key
FK       foreign key
UQ       unique constraint/index
CHECK    database check constraint
partial  WHERE 조건이 붙은 unique/index
APP      여러 row를 함께 봐야 하므로 application transaction에서 검증하는 invariant
```

공통 규칙:

- guild-owned root는 `PK (guild_id, id)` 또는 이에 준하는 `UQ (guild_id, id)`를 가진다.
- child FK는 `id` 하나가 아니라 `(guild_id, parent_id)`를 참조한다.
- 사용자 참가 기록은 가능한 한 `(guild_id, user_id) -> guild_members`를 참조한다.
- API와 repository는 opaque `id`만 받지 않고 항상 `GuildId`를 함께 받는다.
- `created_by`, `updated_by`는 Discord user actor일 때 guild member를 참조한다.
- Bot/System/Migration도 가능한 audit/ledger actor에는 `principal_type`과
  `principal_id`를 사용한다.
- champion ID는 Data Dragon의 stable string slug를 저장한다. 이름과 이미지 URL은 저장하지
  않는다.

모든 `double precision` MMR column은 NaN과 infinity를 거부한다. Migration은 nullable 여부에
맞춰 다음 실제 CHECK 표현을 각 column에 붙인다.

```sql
CHECK (value NOT IN (
  'NaN'::double precision,
  'Infinity'::double precision,
  '-Infinity'::double precision
))

CHECK (value IS NULL OR value NOT IN (
  'NaN'::double precision,
  'Infinity'::double precision,
  '-Infinity'::double precision
))
```

Discord snowflake column은 production write에서 숫자 문자열인지 검증한다. UUIDv7은 DB
default가 아니라 Rust application에서 생성해 HTTP command, DB row, outbox event가 같은 ID를
공유할 수 있게 한다.

## Tenant 격리

Tenant FK 예시:

```sql
FOREIGN KEY (guild_id, series_id)
  REFERENCES series (guild_id, id)
  ON DELETE RESTRICT
```

다음 방식은 금지한다.

```sql
-- guild_id가 서로 달라도 id만 맞으면 참조될 수 있으므로 금지
FOREIGN KEY (series_id) REFERENCES series (id)
```

Guild A URL에서 Guild B ID를 전달하면 repository query가 row를 찾지 못해야 하며 HTTP는
`404`를 반환한다. application 권한 검사뿐 아니라 composite FK와 unique constraint로도
교차 tenant write를 막는다.

## Global identity와 인증

### `users`

```text
user_id          text        PK
identity_kind    text        NOT NULL CHECK DISCORD|DUMMY
display_name     text        NOT NULL
avatar_hash      text        NULL
created_at       timestamptz NOT NULL
updated_at       timestamptz NOT NULL
deleted_at       timestamptz NULL
```

- 서비스 사용자의 전역 root다. `DISCORD` 사용자는 canonical snowflake를, `DUMMY` 사용자는
  `dmy_<UUIDv7>`를 `user_id`로 사용하며 DB constraint가 identity kind와 ID 형식을 함께 검증한다.
- 삭제는 `deleted_at` soft delete다. domain history FK는 `RESTRICT`한다.
- guild nickname은 이 table이 아니라 `guild_members.display_name`에 둔다.

### `user_discord_identities`

```text
user_id       text        PK FK -> users(user_id, identity_kind) CASCADE
discord_id    text        NOT NULL UQ CHECK Discord snowflake
identity_kind text        NOT NULL CHECK DISCORD
created_at    timestamptz NOT NULL
updated_at    timestamptz NOT NULL
```

Discord OAuth와 Bot 입력은 `discord_id`로 사용자를 찾은 뒤 내부 `user_id`로 변환한다. `DUMMY` 사용자는
이 table의 row를 가질 수 없으며 composite FK가 이를 강제한다.

### `oauth_login_attempts`

```text
id                       uuid        PK
state_hash               bytea       NOT NULL UQ
pkce_verifier_ciphertext bytea       NOT NULL
redirect_path            text        NOT NULL
created_at               timestamptz NOT NULL
expires_at               timestamptz NOT NULL
consumed_at              timestamptz NULL
```

- OAuth `state`와 PKCE verifier의 짧은 수명 저장소다.
- `redirect_path`는 검증된 same-origin 상대 경로만 저장한다.
- 만료 또는 사용 완료 row는 주기적으로 hard delete한다.
- OAuth 임시 상태와 로그인 session을 같은 table에 넣지 않는다.

### `user_sessions`

```text
id                uuid        PK
user_id           text        NOT NULL FK -> users.user_id RESTRICT
token_hash        bytea       NOT NULL UQ
csrf_secret_hash  bytea       NOT NULL
rotated_from_id   uuid        NULL FK -> user_sessions.id SET NULL
created_at        timestamptz NOT NULL
last_seen_at      timestamptz NOT NULL
expires_at        timestamptz NOT NULL
revoked_at        timestamptz NULL
```

Indexes:

- `UQ (token_hash)`
- `(user_id, created_at DESC) WHERE revoked_at IS NULL`
- `(expires_at)` cleanup index

`expires_at > now()`를 partial index predicate로 쓰지 않는다. PostgreSQL index predicate에
현재 시각을 넣는 방식은 안전한 migration 기준이 아니다.

### `riot_accounts`

```text
puuid                    text        PK
user_id                  text        NOT NULL FK -> users.user_id RESTRICT
origin                   text        NOT NULL CHECK RIOT|DUMMY
owner_identity_kind      text        GENERATED ALWAYS AS RIOT->DISCORD, DUMMY->DUMMY
platform                 text        NOT NULL       # KR 등
routing_region           text        NOT NULL       # ASIA 등
game_name                text        NOT NULL
tag_line                 text        NOT NULL
normalized_game_name     text        NOT NULL
normalized_tag_line      text        NOT NULL
profile_icon_id          integer     NULL CHECK >= 0
is_main                  boolean     NOT NULL
main_position            text        NULL CHECK lane 또는 FLEX
main_position_updated_at timestamptz NULL
last_synced_at            timestamptz NULL
created_at                timestamptz NOT NULL
updated_at                timestamptz NOT NULL
```

Constraints:

- `FK (user_id, owner_identity_kind) -> users(user_id, identity_kind) RESTRICT`
- partial `UQ (routing_region, normalized_game_name, normalized_tag_line) WHERE origin=RIOT`
- partial `UQ (user_id) WHERE is_main`
- partial `UQ (user_id) WHERE origin=DUMMY`
- `origin=DUMMY`이면 `puuid LIKE 'dummy:%'`이고 `last_synced_at IS NULL`
- account가 하나 이상이면 정확히 하나가 main이라는 최소 1개 규칙은 계정 추가/삭제/메인 변경
  transaction에서 검증한다.
- `game_name`, `tag_line`의 원문은 표시용이고 normalized 값은 lookup용이다.
- account link command input은 `gameName`, `tagLine`, `platform`뿐이다. `puuid`, `routing_region`,
  canonical name/tag, `profile_icon_id`는 Account-v1/profile 응답을 검증한 backend만 기록한다. Client가
  보낸 provider-derived field를 신뢰하거나 upsert에 그대로 전달하지 않는다.
- Riot resolve는 DB transaction 전에 수행한다. 그 뒤 짧은 account-link transaction에서 resolved PUUID와
  normalized Riot ID의 충돌, user ownership, exact-one-main을 함께 검증한다. 이미 다른 user에 연결된
  PUUID는 자동 이전하지 않고 conflict로 거절한다.
- create/list/main/refresh/delete command 응답은 commit 후 같은 transaction 결과로 만든 최신 user account
  snapshot이다. Client가 main account를 추론하거나 mutation 직후 재조회할 필요가 없다.
- `origin=RIOT`은 `identity_kind=DISCORD`, `origin=DUMMY`는 `identity_kind=DUMMY` user만 소유한다.
  Generated owner kind와 composite FK가 application 경계를 우회한 잘못된 소유 관계도 거부한다.
- 합성 account는 Riot provider lookup/refresh, public Riot autocomplete와 실제 PUUID ownership query에서
  제외한다.
- `normalize_riot_id_v1`은 Unicode NFKC, 앞뒤 Unicode whitespace trim, locale-independent Unicode
  case fold 순으로 적용하며 내부 whitespace는 보존한다. PostgreSQL collation/`lower()`에 맡기지
  않고 Rust의 같은 pure function을 create/update/search/import에 사용하고 golden vector를 둔다.
  알고리즘 변경은 기존 row 전체를 재작성하는 명시적 migration 없이는 허용하지 않는다.

### `riot_records_snapshots`, lookup alias와 refresh gate

```text
riot_records_snapshots
  platform, puuid                         PK
  routing_region, game_name, tag_line    NOT NULL
  normalized_game_name/tag_line          NOT NULL
  schema_version                         integer NOT NULL CHECK > 0
  snapshot                               jsonb NOT NULL CHECK object
  refreshed_at, created_at, updated_at   timestamptz NOT NULL

riot_records_lookup_aliases
  platform, normalized_game_name/tag_line  PK
  snapshot_platform, snapshot_puuid        FK -> riot_records_snapshots CASCADE
  resolved_at                              timestamptz NOT NULL

riot_records_refresh_gates
  platform, normalized_game_name/tag_line  PK
  lease_token/acquired_at/until             nullable all-or-none
  created_at, updated_at                     timestamptz NOT NULL
```

- 외부 전적 cache의 canonical key는 `(platform, puuid)`다. Browser에는 Riot ID만 받고 alias table에서
  canonical snapshot을 찾는다.
- `snapshot`은 private Riot integration의 원본 provider payload가 아니라 Axum이 역직렬화하고 검증한
  provider-neutral records DTO 전체다. Schema version을 올릴 때 read adapter 또는 명시적 invalidation을
  함께 배포한다.
- GET cache hit는 만료시키지 않는다. 명시 갱신 또는 필요한 recent page cache miss만 Riot provider를
  호출한다.
- refresh gate는 PostgreSQL clock과 row lock으로 동시 provider 호출을 합치며, 마지막 성공
  `refreshed_at`부터 30초 cooldown을 강제한다. Provider 호출 lease는 시즌 전체 집계 시간을 고려해 5분이고,
  실패 시 snapshot은 보존하고 lease만 해제한다.

### `riot_records_recent_pages`

```text
platform, puuid, page_offset, requested_count  PK
schema_version                                 integer NOT NULL CHECK > 0
page                                           jsonb NOT NULL CHECK object
fetched_at, created_at, updated_at             timestamptz NOT NULL

FK (platform, puuid) -> riot_records_snapshots (platform, puuid) CASCADE
```

더보기 응답은 정확한 `(offset, requested_count)`별 provider-neutral page DTO를 저장한다. 상단 records
snapshot 갱신 성공 시 match ordering이 달라질 수 있으므로 같은 transaction에서 해당 PUUID의 page row를
전부 삭제한다.

### `user_privacy_settings`

```text
user_id                       text        PK FK -> users.user_id CASCADE
public_profile_enabled        boolean     NOT NULL
show_riot_ids                 boolean     NOT NULL
allow_external_record_search boolean     NOT NULL
updated_at                    timestamptz NOT NULL
```

세 boolean은 `/me?tab=privacy`의 현재 UI 계약과 일치한다. 같은 guild 내부의 game/MMR
조회 권한은 이 설정으로 숨기지 않는다.

- `show_riot_ids`는 Discord 사용자와 `riot_accounts`의 연결 관계를 사용자/길드 projection에서 공개할지
  결정한다. Riot ID 자체의 public Riot records 조회와 자동완성에는 적용하지 않는다.
- `allow_external_record_search`는 대상 사용자가 속한 guild 밖의 사용자가 해당 guild의 custom match
  전적에 접근하는 경우에만 적용한다. 같은 guild 구성원의 원장 조회와 public Riot records 조회에는
  적용하지 않는다.
- privacy를 적용한 read model은 false와 설정 row 부재를 같은 비공개 상태로 취급한다. Browser에 privacy
  flag나 PUUID를 보내 판단시키지 않고 공개 가능한 계정 projection 또는 빈 배열만 반환한다.

### `user_guild_preferences`

```text
user_id          text        PK FK -> users.user_id CASCADE
default_guild_id text        NULL
updated_at       timestamptz NOT NULL

FK (default_guild_id, user_id)
  -> guild_members (guild_id, user_id)
```

대표 guild는 단순히 존재하는 guild가 아니라 사용자가 속한 guild여야 한다. membership이
INACTIVE가 되면 기본값을 유지할지 해제할지는 application policy로 처리하되 root redirect는
ACTIVE membership만 사용한다.

### `user_champion_preferences`

```text
user_id      text     NOT NULL FK -> users.user_id CASCADE
lane         text     NOT NULL CHECK canonical lane
position     smallint NOT NULL CHECK 1..10
champion_id  text     NOT NULL

PK (user_id, lane, position)
UQ (user_id, lane, champion_id)
```

- `/me`에서 guild 선택 없이 편집하므로 전역 사용자 설정이다.
- position은 사용자 지정 순서다.
- public lane allowlist는 `TOP|JUNGLE|MID|BOTTOM|SUPPORT`이며 `FLEX`나 임의 문자열을 저장하지 않는다.
- 한 lane replace input은 ordered unique `championIds`, 최대 10개다. 빈 배열은 해당 lane row 전체 삭제다.
- 한 lane의 position이 1부터 빈틈없이 이어지는 규칙은 lane 전체 replace transaction에서
  검증한다.
- replace transaction은 기존 lane row를 교체하고 position을 1부터 다시 부여한 뒤 commit 결과에서 다섯
  lane의 full snapshot을 만든다. Row가 없는 lane도 response에서 빈 배열로 반환한다.

## Development 더미 사용자 관리

### `dummy_user_profiles`

```text
user_id       text        PK FK -> users.user_id RESTRICT CHECK dmy_<UUIDv7>
stable_key    text        NOT NULL UQ CHECK ^[a-z0-9][a-z0-9_-]{0,63}$
state         text        NOT NULL CHECK ACTIVE|ARCHIVED
avatar_seed   text        NULL CHECK trim, 1..128
notes         text        NULL CHECK <= 1000
revision      bigint      NOT NULL CHECK >= 0
created_by    text        NOT NULL FK -> users.user_id RESTRICT
updated_by    text        NOT NULL FK -> users.user_id RESTRICT
created_at    timestamptz NOT NULL
updated_at    timestamptz NOT NULL
```

Profile에는 provenance, 표시 seed, 메모, lifecycle과 revision만 둔다. Privacy, guild membership/MMR,
선호 챔피언, 합성 Riot account, workflow participant를 JSON으로 복제하지 않는다. `created_by`와
`updated_by`는 로그인한 실제 service admin이다.

`ARCHIVED`는 `users.deleted_at` soft delete와 다른 운영 상태다. Archive는 OPEN/CLOSED 일반·경매 모집의
JOINED 참가가 없을 때만 허용하고, ACTIVE 길드 소속을 INACTIVE로 바꾸면서 각 길드의 MEMBERS revision과
감사 로그를 갱신한다. Restore는 profile만 ACTIVE로 바꾸고 길드 소속은 자동 재활성화하지 않는다.

### `dummy_user_groups`

```text
id           uuid        PK
name         text        NOT NULL CHECK trim, 1..100
description  text        NULL CHECK <= 500
revision     bigint      NOT NULL CHECK >= 0
created_by   text        NOT NULL FK -> users.user_id RESTRICT
updated_by   text        NOT NULL FK -> users.user_id RESTRICT
created_at   timestamptz NOT NULL
updated_at   timestamptz NOT NULL

UQ lower(name)
```

### `dummy_user_group_members`

```text
group_id  uuid        NOT NULL FK -> dummy_user_groups.id CASCADE
user_id   text        NOT NULL FK -> dummy_user_profiles.user_id RESTRICT
position  smallint    NOT NULL CHECK 1..100
added_at  timestamptz NOT NULL

PK (group_id, user_id)
UQ (group_id, position)
```

Group은 guild나 workflow에 종속되지 않는 재사용 선택 집합이다. Position은 UI와 기능 배치에서 사용하는
명시적 순서이며 member 전체 교체 transaction에서 1부터 다시 부여한다.

### `dummy_user_audit_log`

```text
id              uuid        PK
actor_user_id   text        NOT NULL FK -> users.user_id RESTRICT
action          text        NOT NULL CHECK 1..100
target_type     text        NOT NULL CHECK DUMMY_USER|DUMMY_USER_GROUP|GUILD_MEMBER
target_id       text        NOT NULL CHECK 1..256
request_id      text        NOT NULL CHECK 1..128
before_summary  jsonb       NULL
after_summary   jsonb       NULL
created_at      timestamptz NOT NULL
```

이 table은 append-only다. 길드 소속이나 모집 참가처럼 기존 guild 업무 원장에도 영향을 주는 mutation은
해당 `admin_audit_log`도 함께 기록한다.

## Guild root와 시즌

### `guilds`

```text
guild_id      text        PK
discord_name  text        NOT NULL
icon_hash     text        NULL
status        text        NOT NULL CHECK INSTALLED|SETUP_REQUIRED|ACTIVE|DISABLED|LEFT
installed_by  text        NULL FK -> users.user_id RESTRICT
installed_at  timestamptz NOT NULL
activated_at  timestamptz NULL
left_at       timestamptz NULL
updated_at    timestamptz NOT NULL
```

Bot이 제거되어도 row와 domain history는 삭제하지 않고 `LEFT`로 전이한다.

### `guild_members`

```text
guild_id          text        NOT NULL FK -> guilds.guild_id RESTRICT
user_id           text        NOT NULL FK -> users.user_id RESTRICT
display_name      text        NOT NULL
status            text        NOT NULL CHECK ACTIVE|INACTIVE
main_lane         text        NULL CHECK canonical lane
first_seen_at     timestamptz NOT NULL
registered_at     timestamptz NULL
discord_joined_at timestamptz NULL
last_active_at    timestamptz NULL
last_synced_at    timestamptz NULL
inactivated_at    timestamptz NULL

PK (guild_id, user_id)
```

- admin의 멤버 상태 원장이다.
- 운영자 여부는 별도 boolean으로 중복 저장하지 않고 현재 `guild_config.operator_role_id`와
  동기화된 Discord role membership으로 판정한다.
- `registered`는 `registered_at IS NOT NULL`로 계산한다. ACTIVE/INACTIVE와 Riot 계정 연결 여부는
  별도 개념이며 합치지 않는다.
- guild에서 실행한 `/등록`이 성공하면 Discord snowflake로 `users`를 확인한 뒤
  `(guild_id, user_id)` row와 PUUID 기반 `riot_accounts` 연결을 같은 짧은 transaction에서 확정한다.
  새 member는 ACTIVE와 `registered_at`을 기록한다. 기존 ACTIVE member는 status를 유지하면서 Discord
  전역/길드 표시 정보, 활동·동기화 시각과 role snapshot을 최신 interaction 기준으로 갱신한다. 기존
  INACTIVE member의 status와 `inactivated_at`은 변경하지 않고 command를 거절한다.
- 같은 interaction 또는 같은 사용자의 반복 `/등록`은 멱등적으로 처리한다. `PK (guild_id, user_id)`와
  PUUID/user ownership constraint를 우회해 중복 소속이나 중복 계정을 만들지 않는다.
- admin member DTO의 `mainLane`과 `lastActiveAt`은 각각 nullable 원본을 그대로 사용한다. 값이 없는
  member에 임의 lane이나 timestamp를 합성하지 않는다.
- 상대 시간 label과 한국어 role/status label은 frontend에서 timestamp/canonical enum으로 계산한다.
- OPERATOR 전용 admin projection의 `riotId`는 연결 상태 확인을 위한 main account
  `game_name + '#' + tag_line`이며 `show_riot_ids`를 적용하지 않는다. null은 실제 미연결을 뜻한다.
  Public/guild records projection은 별도로 `show_riot_ids`를 적용하고 PUUID는 어느 projection에도
  포함하지 않는다.

### `guild_member_discord_roles`

```text
guild_id       text        NOT NULL
user_id        text        NOT NULL
role_id        text        NOT NULL
last_synced_at timestamptz NOT NULL

PK (guild_id, user_id, role_id)
FK (guild_id, user_id) -> guild_members (guild_id, user_id) CASCADE
```

Discord role sync cache다. 현재 MVP browser admin 권한은 ACTIVE `guild_members`와 이 table에서
`guild_config.operator_role_id`가 일치하는지로 판정한다. 별도 TTL이나 요청 시 Discord REST 재검증은 아직
구현하지 않았으므로 더 강한 freshness 보장을 전제로 하지 않는다. Bot이 member/role sync를 수행할 때
`last_synced_at`을 갱신한다.

### `guild_config`

```text
guild_id               text        PK FK -> guilds.guild_id RESTRICT
display_name           text        NOT NULL
operator_role_id       text        NOT NULL
recruitment_channel_id text        NULL
entry_channel_id       text        NULL
result_channel_id      text        NULL
log_channel_id         text        NULL
is_externally_public   boolean     NOT NULL
revision               bigint      NOT NULL CHECK >= 0
updated_by             text        NULL
updated_at             timestamptz NOT NULL

FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
```

- 현재 UI는 운영자 role 하나만 지원하므로 `guild_operator_roles` table을 만들지 않는다.
- `current_season_id`를 두지 않는다. 현재 시즌의 source of truth는 `seasons.status=ACTIVE`다.
- admin settings의 `settingsRevision`은 이 row의 `revision`이다.
- `recruitment_channel_id`는 모집 메시지, `entry_channel_id`는 밸런스 결과와 확정 엔트리,
  `result_channel_id`는 일반 시리즈 최종 결과, `log_channel_id`는 운영 알림의 목적지다.
  physical column은 legacy row 보존을 위해 nullable이다. 그러나 신규 `/초기설정`과 Browser settings PUT은
  `recruitment_channel_id`를 반드시 요구한다. legacy `NULL` row는 GET으로 읽어 복구할 수 있지만 다음 저장에는
  모집 채널을 선택해야 한다. `entry_channel_id`, `result_channel_id`, `log_channel_id`는 미설정을 허용한다.
  `result_channel_id=NULL`이면 최종 결과 notification을 생략하며, `entry_channel_id`를 결과 알림 fallback으로
  사용하지 않는다.
- Browser settings PUT은 이 row를 lock하고 현재 `operator_role_id`로 요청자를 먼저 권한 검사한 뒤
  `expectedRevision`을 비교한다. 성공하면 설정, revision, `updated_by`, `updated_at`과
  `GUILD_SETTINGS_UPDATED` audit를 한 transaction에서 갱신한다.

### `guild_discord_roles`

```text
guild_id  text        NOT NULL FK -> guilds.guild_id CASCADE
role_id   text        NOT NULL
name      text        NOT NULL CHECK trimmed length 1..100
position  integer     NOT NULL CHECK >= 0
managed   boolean     NOT NULL
updated_at timestamptz NOT NULL

PK (guild_id, role_id)
CHECK (role_id <> guild_id)
```

길드 설정의 운영자 역할 선택 catalog다. `guild_member_discord_roles`가 사용자별 role membership을
저장하는 것과 달리 이 table은 Discord 길드에 존재하는 role 목록을 저장한다. managed role도 동기화해
원본 snapshot은 보존하지만 settings GET과 새 운영자 role 검증에서는 `managed=false`만 사용한다.

### `guild_discord_channels`

```text
guild_id   text        NOT NULL FK -> guilds.guild_id CASCADE
channel_id text        NOT NULL
name       text        NOT NULL CHECK trimmed length 1..100
parent_name text       NULL CHECK trimmed length 1..100
position   integer     NOT NULL CHECK >= 0
kind       text        NOT NULL CHECK TEXT|ANNOUNCEMENT
updated_at timestamptz NOT NULL

PK (guild_id, channel_id)
```

Bot이 볼 수 있고 메시지를 보낼 수 있는 Discord text/announcement channel catalog다. 설정 GET은
`position`, 이름, ID 순으로 정렬해 반환한다. 설정 PUT에서 기존 값과 달라진 non-null channel ID는 이
table에 있어야 하며 null은 `설정 안 함`을 뜻한다.

### `guild_discord_catalog_state`

```text
guild_id  text        PK FK -> guilds.guild_id CASCADE
synced_at timestamptz NOT NULL
```

Bot service-principal의 catalog PUT은 `guild_config`를 lock하고 role/channel row를 전체 교체한 뒤 이
시각을 갱신한다. 아직 한 번도 동기화되지 않은 길드는 settings snapshot의 `catalogUpdatedAt=null`과 빈
목록을 반환한다. Discord에서 삭제되어 catalog에서 사라진 기존 설정값은 즉시 임의 변경하지 않는다.
그 값을 그대로 저장하는 것은 허용하지만 다른 누락 ID로 변경하는 요청은 거부해 운영자가 명시적으로
유효한 catalog 항목이나 null을 선택하게 한다.

### `guild_admin_state`

```text
guild_id   text        NOT NULL FK -> guilds.guild_id RESTRICT
scope      text        NOT NULL CHECK MEMBERS|OPERATIONS
revision   bigint      NOT NULL CHECK >= 0
updated_by text        NULL
updated_at timestamptz NOT NULL

PK (guild_id, scope)
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
```

- `/admin?tab=members`는 `scope=MEMBERS` row를 사용한다.
- `scope=OPERATIONS`는 후속 롤백 기능용 예약 값이다. MVP 관리 화면과 route에서는 노출하지 않는다.
- settings는 `guild_config.revision`을 사용한다.
- 세 revision을 하나로 합치지 않으며 MEMBERS/OPERATIONS가 서로 row lock을 공유하지 않는다.
- 각 snapshot의 409 metadata는 해당 scope row의 `updated_at`, `updated_by`에서 반환한다.
- Admin member GET과 mutation 응답은 이 metadata와 전체 member projection을 함께 반환한다. Mutation
  성공 시 client가 revision이나 단일 member를 추론하지 않고 full snapshot으로 교체한다.
- 기존 guild에 MEMBERS row를 backfill하는 migration은 요구하지 않는다. Row가 없으면 GET은
  `revision=0`, `COALESCE(guild_config.updated_at, guilds.updated_at)`과 `guild_config.updated_by`
  metadata를 projection하고 DB를 변경하지 않는다. 첫 member mutation이 actor/현재 시각으로 revision 0 row를
  `INSERT ... ON CONFLICT DO NOTHING`한 뒤 lock한다. 따라서 기존 guild와 concurrent 첫 mutation이 같은
  경로로 합류한다.

### `seasons`

```text
guild_id    text        NOT NULL FK -> guilds.guild_id RESTRICT
id          uuid        NOT NULL
name        text        NOT NULL
status      text        NOT NULL CHECK ACTIVE|CLOSED
started_at  timestamptz NOT NULL
ended_at    timestamptz NULL
created_by  text        NULL
created_at  timestamptz NOT NULL

PK (guild_id, id)
FK (guild_id, created_by) -> guild_members (guild_id, user_id) RESTRICT
partial UQ (guild_id) WHERE status='ACTIVE'
CHECK ((status='ACTIVE' AND ended_at IS NULL)
    OR (status='CLOSED' AND ended_at IS NOT NULL))
```

시즌은 Web settings payload에 포함하지 않으며 `GET/POST /api/guilds/{guildId}/seasons`의 별도
query/command로 조회하고 전환한다. GET projection은 현재 ACTIVE 시즌과 최신 종료순 CLOSED 시즌에 대해
확정 game에 참여한 distinct 사용자 수, 삭제되지 않은 COMPLETED 일반 시리즈/경매 내전 수, CONFIRMED game
수, 경매 10인/20인 수와 최근 `confirmed_at`을 집계한다. `created_by`가 없으면 API의 `createdBy`는
null이다.

`ROTATE_SEASON`은 현재 ACTIVE season row를 lock하고 `expectedCurrentSeasonId`를 비교한 뒤
CLOSED/ended_at으로 바꾸고 새 ACTIVE season을 생성한다. Audit와 idempotency response까지 같은
transaction에 저장한다. 과거 game, auction, `user_lane_mmr`와 ledger는 유지하며 새 시즌 MMR row는 첫
조회/참가 시 기본 1500으로 생성한다. 기존 시즌을 비우는 reset command는 만들지 않는다. 향후 Bot
`/새시즌`은 같은 application command를 호출하되 Browser session/CSRF가 아닌 Bot service principal과
Discord interaction ID를 사용한다.

### `guild_counters`

```text
guild_id    text   NOT NULL FK -> guilds.guild_id RESTRICT
counter_key text   NOT NULL CHECK RECRUITMENT|SERIES|AUCTION_RECRUITMENT|AUCTION_TOURNAMENT
next_value  bigint NOT NULL CHECK > 0

PK (guild_id, counter_key)
```

사람에게 보이는 `#214`, `A-118` 같은 번호를 tenant별로 원자적으로 배정한다. UUID를 화면용
순번으로 재사용하거나 `MAX(display_number)+1`을 계산하지 않는다.

## 일반 모집과 엔트리

### `recruitments`

```text
guild_id       text        NOT NULL
id             uuid        NOT NULL
season_id      uuid        NOT NULL
display_number bigint      NOT NULL CHECK > 0
title          text        NOT NULL
target_count   smallint    NOT NULL DEFAULT 10 CHECK = 10
best_of        smallint    NOT NULL DEFAULT 3 CHECK IN (1,3,5)
hard_fearless  boolean     NOT NULL DEFAULT false
status         text        NOT NULL CHECK OPEN|CLOSED|CONVERTED|CANCELLED
revision       bigint      NOT NULL CHECK >= 0
channel_id     text        NULL
message_id     text        NULL
created_by     text        NOT NULL
created_at     timestamptz NOT NULL
updated_by     text        NULL
updated_at     timestamptz NOT NULL
closed_at      timestamptz NULL
converted_at   timestamptz NULL
cancelled_at   timestamptz NULL

PK (guild_id, id)
UQ (guild_id, display_number)
UQ (guild_id, season_id, id)
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, created_by) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
```

현재 production route, records DTO, MMR workflow는 5v5 일반 내전만 지원한다. 재사용 component의
가변 `targetCount` prop은 제품 계약이 아니다. 다른 인원 모드는 records와 MMR 계약까지 실제로
추가할 때 별도 migration으로 확장한다.

모집 생성 command가 `best_of`와 `hard_fearless`를 함께 기록한다. `CLOSED -> CONVERTED` 전환은 이 값을
새 `series` row에 복사하며, 이후 모집이나 픽밴 workspace에서 값을 수정하지 않는다. 모집과 series에
동일 필드를 두는 것은 source 설정과 경기 기록 snapshot의 역할을 분리하기 위한 의도적인 중복이다.

### `recruitment_participants`

```text
guild_id       text        NOT NULL
recruitment_id uuid        NOT NULL
user_id        text        NOT NULL
status         text        NOT NULL CHECK JOINED|WITHDRAWN
joined_at      timestamptz NOT NULL
withdrawn_at   timestamptz NULL

PK (guild_id, recruitment_id, user_id)
FK (guild_id, recruitment_id) -> recruitments (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
```

참가 취소를 물리 삭제하지 않는다. 재참가는 같은 row를 `JOINED`로 되돌리고 시각을 갱신한다.

### `recruitment_preferred_lanes`

```text
guild_id       text     NOT NULL
recruitment_id uuid     NOT NULL
user_id        text     NOT NULL
priority       smallint NOT NULL CHECK 1..5
lane           text     NOT NULL CHECK canonical lane

PK (guild_id, recruitment_id, user_id, priority)
UQ (guild_id, recruitment_id, user_id, lane)
FK (guild_id, recruitment_id, user_id)
  -> recruitment_participants (guild_id, recruitment_id, user_id) CASCADE
```

첫 번째 lane이 모집 시점의 main preference다. 선호가 없으면 row가 없다.

### `recruitment_entry_drafts`

```text
guild_id       text        NOT NULL
recruitment_id uuid        NOT NULL
revision       bigint      NOT NULL CHECK >= 0
updated_by     text        NULL
updated_at     timestamptz NOT NULL

PK (guild_id, recruitment_id)
FK (guild_id, recruitment_id) -> recruitments (guild_id, id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
```

Entry workspace의 aggregate revision owner다. `recruitments.revision`은 모집 상태/roster lifecycle의
owner이고, 이 revision은 배치와 entry workspace의 owner다. 제한적 realtime에서 draft invalidate는
`guild:{guildId}:recruitment-entry-draft:{recruitmentId}` topic과 이 draft revision을 사용하며,
recruitment topic에는 recruitment revision만 넣는다. 참가/취소로 두 snapshot이 함께 바뀌면 해당 command는
두 revision을 같은 transaction에서 독립적으로 갱신한다. outbox는 composite entry route를 다시 읽혀야 하는
aggregate의 topic 하나 또는 둘을 적재할 수 있지만, topic마다 그 owner revision만 사용한다.
배정된 참가자가 withdraw되거나 강제 제거되면 assignment를 같은 transaction에서 삭제한다.
Series 생성 시에도 10개 assignment가 모두 현재 JOINED participant인지 다시 검증한다.

### `recruitment_entry_assignments`

```text
guild_id       text NOT NULL
recruitment_id uuid NOT NULL
user_id        text NOT NULL
team           text NOT NULL CHECK TEAM_1|TEAM_2
lane           text NOT NULL CHECK canonical lane

PK (guild_id, recruitment_id, user_id)
UQ (guild_id, recruitment_id, team, lane)
FK (guild_id, recruitment_id) -> recruitment_entry_drafts (...) CASCADE
FK (guild_id, recruitment_id, user_id) -> recruitment_participants (...) RESTRICT
```

미배정 후보는 assignment row가 없는 JOINED participant다. 전체 JSON replace 대신 한 participant
이동/교환 command로 수정한다.

## 일반 시리즈

### `series`

```text
guild_id             text        NOT NULL
id                   uuid        NOT NULL
season_id            uuid        NOT NULL
source_recruitment_id uuid       NOT NULL
display_number       bigint      NOT NULL CHECK > 0
title                text        NOT NULL
best_of              smallint    NOT NULL CHECK IN (1,3,5)
hard_fearless        boolean     NOT NULL DEFAULT false
status               text        NOT NULL CHECK IN_PROGRESS|COMPLETED|CANCELLED
winning_team         text        NULL CHECK TEAM_1|TEAM_2
terminal_reason      text        NULL CHECK SCORE_REACHED|EARLY_WIN|EARLY_DRAW|CANCELLED
revision             bigint      NOT NULL CHECK >= 0
started_at           timestamptz NOT NULL
ended_at             timestamptz NULL
created_by           text        NULL
end_card_channel_id  text        NULL
end_card_message_id  text        NULL
updated_by           text        NULL
updated_at           timestamptz NOT NULL
deleted_at           timestamptz NULL

PK (guild_id, id)
UQ (guild_id, display_number)
UQ (guild_id, season_id, id)
partial UQ (guild_id, source_recruitment_id)
  WHERE status<>'CANCELLED' AND deleted_at IS NULL
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, season_id, source_recruitment_id)
  -> recruitments (guild_id, season_id, id) RESTRICT
FK (guild_id, created_by) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
CHECK ((status='IN_PROGRESS' AND winning_team IS NULL AND terminal_reason IS NULL AND ended_at IS NULL)
    OR (status='COMPLETED' AND terminal_reason IN ('SCORE_REACHED','EARLY_WIN') AND
        winning_team IS NOT NULL AND ended_at IS NOT NULL)
    OR (status='COMPLETED' AND terminal_reason='EARLY_DRAW' AND
        winning_team IS NULL AND ended_at IS NOT NULL)
    OR (status='CANCELLED' AND terminal_reason='CANCELLED' AND
        winning_team IS NULL AND ended_at IS NOT NULL))
```

기존 v3의 `series.id == recruitments.id` 규칙은 폐기한다. source recruitment와 series는 서로
다른 UUID를 갖는다. 같은 모집에는 non-cancelled series가 최대 하나이며 생성 되돌리기로 기존
series가 CANCELLED되면 재시도는 새 series ID를 발급한다.

Series 생성 되돌리기는 confirmed game이 없으면 기존 series를 곧바로 `CANCELLED`로,
source recruitment를 `CLOSED`로 되돌린다. confirmed game이 있으면 마지막 game부터 ledger tail을
검증해 `VOIDED`/MMR reversal을 역순으로 완료한 뒤 같은 lifecycle revert를 수행한다. 새 전환
command는 기존 entry draft에서 새 series ID를 만들며, 기존 row를 재사용하거나 물리 삭제하지 않는다.
신규 series는 created_by가 필수이며 nullable은 v3의 actor 미기록 row만 허용한다.
운영자 조기 종료의 동률 결과는 `COMPLETED + EARLY_DRAW + winning_team=NULL`로 보존한다. 일반 score
달성 완료와 운영자 지정 조기 승리를 같은 nullable winner로 추측하지 않고 terminal_reason으로
구분한다.

완료 시 configured `result_channel_id`에만 outbox 기반 one-shot 결과 카드가 비동기로 발행된다. 현재
`end_card_channel_id`와 `end_card_message_id`는 이후 결과 정정 시 기존 카드 edit/delete를 위한 예약
필드이며, 첫 결과 발행의 generic acknowledgement는 이 두 field를 bind하지 않는다.
신규 `SERIES_RESULT_CREATE` payload는 `schemaVersion=2`이며 roster와 confirmed set별 pick/ban을 고정한다.
기존 `schemaVersion=1` 대기 row는 payload를 변환하지 않고 legacy renderer로 그대로 delivery하여 outbox
deduplication/replay 의미를 바꾸지 않는다.

`hard_fearless`는 모집 생성 시 선택되어 series로 복사되는 immutable 규칙 snapshot이며 기본값은
`false`다. `true`이면 이전 confirmed set에서 **픽된** 챔피언을 후속 set에서 다시 픽할 수 없다. 이전
ban은 막지 않는다. 설정과 관계없이 같은 set 안에서는 pick과 ban을 합쳐 같은 챔피언을 두 번 사용할 수
없다.

### `series_participants`

```text
guild_id    text             NOT NULL
series_id   uuid             NOT NULL
user_id     text             NOT NULL
team        text             NOT NULL CHECK TEAM_1|TEAM_2
lane        text             NOT NULL CHECK canonical lane
mmr_at_lock       double precision NULL
mmr_snapshot_origin text           NOT NULL CHECK CAPTURED|INFERRED|UNAVAILABLE

PK (guild_id, series_id, user_id)
UQ (guild_id, series_id, team, lane)
FK (guild_id, series_id) -> series (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
CHECK ((mmr_snapshot_origin IN ('CAPTURED','INFERRED') AND mmr_at_lock IS NOT NULL)
    OR (mmr_snapshot_origin='UNAVAILABLE' AND mmr_at_lock IS NULL))
```

시리즈 생성 시 entry assignment와 당시 MMR을 동결한다. 이후 사용자의 main lane 또는 MMR이
바뀌어도 과거 roster가 바뀌지 않는다. 신규 생성은 `CAPTURED`와 non-null MMR만 허용한다.
legacy에서 복원할 수 없는 값만 `INFERRED` 또는 `UNAVAILABLE`을 사용한다.

Pick/ban은 별도 `series_pickban_drafts` JSON table을 만들지 않는다. 일반/경매가 공유하는
정규화된 `game_inputs`가 각 set의 draft이며 `series.revision`이 협업 revision owner다.

## 경매 모집과 준비

### `auction_recruitments`

```text
guild_id       text        NOT NULL
id             uuid        NOT NULL
season_id      uuid        NOT NULL
display_number bigint      NOT NULL CHECK > 0
title          text        NOT NULL
format         text        NOT NULL CHECK TEN_PERSON|TWENTY_PERSON
target_count   smallint    NOT NULL CHECK IN (10,20)
hard_fearless  boolean     NOT NULL DEFAULT false
semifinal_best_of smallint  NULL CHECK IN (1,3,5)
final_best_of  smallint    NOT NULL DEFAULT 3 CHECK IN (1,3,5)
status         text        NOT NULL CHECK OPEN|CLOSED|CONVERTED|CANCELLED
revision       bigint      NOT NULL CHECK >= 0
channel_id     text        NULL
message_id     text        NULL
created_by     text        NOT NULL
created_at     timestamptz NOT NULL
updated_by     text        NULL
updated_at     timestamptz NOT NULL
closed_at      timestamptz NULL
converted_at   timestamptz NULL
cancelled_at   timestamptz NULL

PK (guild_id, id)
UQ (guild_id, display_number)
UQ (guild_id, season_id, id)
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, created_by) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
CHECK ((format='TEN_PERSON' AND target_count=10)
    OR (format='TWENTY_PERSON' AND target_count=20))
CHECK ((format='TEN_PERSON' AND semifinal_best_of IS NULL)
    OR (format='TWENTY_PERSON' AND semifinal_best_of IN (1,3,5)))
```

Discord `/경매모집`과 Web `/g/{guildId}/auction`은 생성 주체만 다르고 같은 create transaction을 사용한다.
요청은 `format`, `final_best_of`, `hard_fearless`와 20인일 때 `semifinal_best_of`를 정하며 `target_count`는
format에서 파생한다. 10인은 `semifinal_best_of=NULL`이고, 20인은 두 준결승이 같은 값을 공유한다. 생략한
호환 요청은 BO3/일반으로 정규화한다. transaction은 ACTIVE guild member와 현재 season/counter를 lock한 뒤 OPEN
`auction_recruitments`, `auction_setup_drafts(revision=0)`, format에 맞는 빈 `auction_setup_teams`
(TEN_PERSON 1..2, TWENTY_PERSON 1..4)를 함께 만든다. 어느 생성 주체든 configured
recruitment channel이 없으면 전체를 rollback하며, 있으면 같은 transaction에
`AUCTION_RECRUITMENT_MESSAGE_CREATE` notification row를 적재한다. `channel_id`와 `message_id`는 Bot typed
acknowledgement가 delivery 상태와 함께 원자적으로 bind하기 전까지 NULL이다.

### `auction_recruitment_participants`

```text
guild_id       text             NOT NULL
recruitment_id uuid             NOT NULL
user_id        text             NOT NULL
status         text             NOT NULL CHECK JOINED|WITHDRAWN
declared_lane  text             NULL CHECK canonical lane
mmr_at_join    double precision NULL
snapshot_origin text            NOT NULL CHECK CAPTURED|INFERRED|UNAVAILABLE
joined_at      timestamptz      NOT NULL
withdrawn_at   timestamptz      NULL

PK (guild_id, recruitment_id, user_id)
FK (guild_id, recruitment_id) -> auction_recruitments (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
CHECK ((snapshot_origin IN ('CAPTURED','INFERRED')
        AND declared_lane IS NOT NULL AND mmr_at_join IS NOT NULL)
    OR (snapshot_origin='UNAVAILABLE' AND (declared_lane IS NULL OR mmr_at_join IS NULL)))
```

`declared_lane`은 입찰 화면의 참고 lane이다. 실제 match lane은
`auction_match_lineups`에서 별도로 확정한다.

경매 신규 참가 시 Bot은 guild main lane을 기본값으로 보여주고 lane select를 받는다. main
lane이 없는 사용자는 lane 선택 없이 참가를 확정할 수 없다. `mmr_at_join`은 선택 lane의 현재
시즌 MMR이며 row가 아직 없으면 v3 기본값 1500을 `CAPTURED`로 snapshot한다. nullable은 legacy
import의 복원 불가능한 과거 row만 허용한다.

### `auction_setup_drafts`

```text
guild_id       text        NOT NULL
recruitment_id uuid        NOT NULL
revision       bigint      NOT NULL CHECK >= 0
updated_by     text        NULL
updated_at     timestamptz NOT NULL

PK (guild_id, recruitment_id)
FK (guild_id, recruitment_id) -> auction_recruitments (guild_id, id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
```

현재 `/auction/recruitments/{id}/draft`는 CLOSED 모집의 실제 확정 명단을 읽는 stage-gate다. 팀장과
시작 point를 저장하는 mutation과 tournament 생성은 별도 vertical slice로 남아 있으며, 그 전에는 fixture
route나 임시 Valkey 상태를 사용하지 않는다.

### `auction_setup_teams`

```text
guild_id       text    NOT NULL
recruitment_id uuid    NOT NULL
team_index     smallint NOT NULL CHECK 1..4
team_name      text    NOT NULL
captain_user_id text   NULL
initial_points integer NOT NULL CHECK > 0

PK (guild_id, recruitment_id, team_index)
UQ (guild_id, recruitment_id, captain_user_id)
FK (guild_id, recruitment_id) -> auction_setup_drafts (...) CASCADE
FK (guild_id, recruitment_id, captain_user_id)
  -> auction_recruitment_participants (guild_id, recruitment_id, user_id) RESTRICT
```

향후 setup mutation의 APP invariant:

- TEN_PERSON은 정확히 2팀, TWENTY_PERSON은 정확히 4팀이다.
- 모든 captain은 JOINED participant이며 서로 달라야 한다.
- tournament 생성은 필요한 captain/point가 모두 채워진 snapshot에서만 가능하다.
- 참가/취소는 `auction_recruitments.revision`과 `auction_setup_drafts.revision`을 같은 transaction에서
  올린다. 현재 Bot self-service 참가/취소가 이 규칙을 적용한다.
- 선택된 captain이 취소/제거되면 해당 setup team의 captain을 NULL로 비우고 draft revision을
  올린다. 이후 captain은 frontend 선택 순서처럼 앞 team으로 당기며 team index별 시작 point와
  team name은 유지한다.

## 경매 tournament와 입찰

### `auction_tournaments`

```text
guild_id              text        NOT NULL
id                    uuid        NOT NULL
season_id             uuid        NOT NULL
source_recruitment_id uuid        NOT NULL
display_number        bigint      NOT NULL CHECK > 0
title                 text        NOT NULL
format                text        NOT NULL CHECK TEN_PERSON|TWENTY_PERSON
hard_fearless         boolean     NOT NULL DEFAULT false
semifinal_best_of     smallint    NULL CHECK IN (1,3,5)
final_best_of         smallint    NOT NULL DEFAULT 3 CHECK IN (1,3,5)
status                text        NOT NULL CHECK BIDDING|BRACKET_SETUP|IN_GAME|COMPLETED|CANCELLED
revision              bigint      NOT NULL CHECK >= 0
bracket_locked_at     timestamptz NULL
bracket_locked_by     text        NULL
champion_team_id      uuid        NULL
started_at            timestamptz NOT NULL
ended_at              timestamptz NULL
created_by            text        NOT NULL
updated_by            text        NULL
updated_at            timestamptz NOT NULL
end_card_channel_id   text        NULL
end_card_message_id   text        NULL
deleted_at            timestamptz NULL

PK (guild_id, id)
UQ (guild_id, display_number)
UQ (guild_id, season_id, id)
partial UQ (guild_id, source_recruitment_id)
  WHERE status<>'CANCELLED' AND deleted_at IS NULL
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, season_id, source_recruitment_id)
  -> auction_recruitments (guild_id, season_id, id) RESTRICT
FK (guild_id, created_by) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, bracket_locked_by) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
CHECK ((status IN ('BIDDING','BRACKET_SETUP','IN_GAME') AND
        champion_team_id IS NULL AND ended_at IS NULL) OR
       (status='COMPLETED' AND champion_team_id IS NOT NULL AND ended_at IS NOT NULL) OR
       (status='CANCELLED' AND champion_team_id IS NULL AND ended_at IS NOT NULL))
CHECK ((format='TEN_PERSON' AND semifinal_best_of IS NULL)
    OR (format='TWENTY_PERSON' AND semifinal_best_of IN (1,3,5)))
```

`champion_team_id` FK는 `auction_teams` 생성 뒤 다음 composite constraint로 추가한다.

```text
FK (guild_id, id, champion_team_id)
  -> auction_teams (guild_id, tournament_id, id) RESTRICT
```

기존 v3의 tournament/recruitment 동일 ID 규칙은 폐기한다.

Tournament 생성은 source 모집의 세 규칙을 불변 snapshot으로 복사한다. 그 뒤 match materialization은
TEN_PERSON `FINAL`에 `final_best_of`, TWENTY_PERSON `SEMIFINAL_A/B`에 `semifinal_best_of`, `FINAL`에
`final_best_of`를 넣는다. `hard_fearless`도 각 match에 복사하며, 사용 금지 범위는 같은 match의 이전 확정
세트로만 제한한다. 준결승 A의 픽이 준결승 B나 결승까지 금지되지는 않는다.

Tournament 생성 되돌리기는 bidding action이 시작되기 전만 허용한다. 기존 tournament를
CANCELLED로, source auction recruitment를 CLOSED로 되돌리고 setup draft를 다시 편집 가능하게
한다. 재시작은 새 tournament ID를 만든다.

### `auction_teams`

```text
guild_id       text        NOT NULL
tournament_id  uuid        NOT NULL
id             uuid        NOT NULL
team_index     smallint    NOT NULL CHECK 1..4
team_name      text        NOT NULL
initial_points integer     NOT NULL CHECK > 0
current_points integer     NOT NULL CHECK >= 0
point_revision bigint      NOT NULL CHECK >= 0
created_at     timestamptz NOT NULL

PK (guild_id, tournament_id, id)
UQ (guild_id, tournament_id, team_index)
FK (guild_id, tournament_id) -> auction_tournaments (guild_id, id) RESTRICT
```

현재 point는 빠른 조회와 lock을 위한 summary다. 모든 증감은
`auction_point_ledger`와 같은 transaction에서 기록한다.

### `auction_team_members`

```text
guild_id           text             NOT NULL
tournament_id      uuid             NOT NULL
team_id            uuid             NOT NULL
id                 uuid             NOT NULL
user_id            text             NOT NULL
roster_slot        smallint         NOT NULL CHECK 1..5
acquired_via       text             NOT NULL CHECK CAPTAIN|BID|MANUAL
declared_lane      text             NULL CHECK canonical lane
mmr_at_acquisition double precision NULL
snapshot_origin    text             NOT NULL CHECK CAPTURED|INFERRED|UNAVAILABLE
acquisition_action_id uuid          NULL
acquisition_action_kind text        NULL CHECK AWARD_BID|ASSIGN_CURRENT|ASSIGN_UNSOLD
joined_at          timestamptz      NOT NULL
released_at        timestamptz      NULL

PK (guild_id, tournament_id, id)
UQ (guild_id, tournament_id, team_id, id, user_id)
FK (guild_id, tournament_id, team_id) -> auction_teams (...) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
partial UQ (guild_id, tournament_id, user_id) WHERE released_at IS NULL
partial UQ (guild_id, tournament_id, team_id, roster_slot) WHERE released_at IS NULL
partial UQ (guild_id, tournament_id, team_id)
  WHERE acquired_via='CAPTAIN' AND released_at IS NULL
partial UQ (guild_id, tournament_id, acquisition_action_id)
  WHERE acquisition_action_id IS NOT NULL
CHECK ((acquired_via='CAPTAIN' AND acquisition_action_id IS NULL AND
        acquisition_action_kind IS NULL)
    OR (acquired_via='BID' AND acquisition_action_id IS NOT NULL AND
        acquisition_action_kind='AWARD_BID')
    OR (acquired_via='MANUAL' AND acquisition_action_id IS NOT NULL AND
        acquisition_action_kind IN ('ASSIGN_CURRENT','ASSIGN_UNSOLD')))
CHECK ((snapshot_origin IN ('CAPTURED','INFERRED')
        AND declared_lane IS NOT NULL AND mmr_at_acquisition IS NOT NULL)
    OR (snapshot_origin='UNAVAILABLE'
        AND (declared_lane IS NULL OR mmr_at_acquisition IS NULL)))
```

- captain도 `CAPTAIN` member row로 반드시 생성한다.
- captain 여부는 `acquired_via='CAPTAIN'`에서 파생하며 별도 boolean으로 중복 저장하지 않는다.
- 낙찰 point는 `acquisition_action_id -> AWARD_BID.points`에서 읽으며 member row에 중복 저장하지
  않는다.
- role별 한 명 제약은 두지 않는다. 경매 roster는 5명 정원이고 실제 lane은 match lineup에서
  정한다.
- 한 팀에 정확히 captain 한 명이라는 최소 1개 규칙은 tournament 생성 transaction이 검증한다.
- BID member의 acquisition action은 같은 target/team의 AWARD_BID여야 하고 MANUAL member는 같은
  target/team의 ASSIGN_CURRENT 또는 ASSIGN_UNSOLD여야 한다.
- 하나의 acquisition action은 정확히 한 member history row에서만 사용할 수 있다. 방출 후에도
  기존 action ID를 새 member row에 재사용하지 않는다.

### `auction_bid_candidates`

```text
guild_id       text             NOT NULL
tournament_id  uuid             NOT NULL
user_id        text             NOT NULL
declared_lane  text             NULL CHECK canonical lane
mmr_at_start   double precision NULL
snapshot_origin text            NOT NULL CHECK CAPTURED|INFERRED|UNAVAILABLE
queue_position bigint           NOT NULL CHECK >= 0
state          text             NOT NULL CHECK QUEUED|ACTIVE|UNSOLD|ASSIGNED
draw_count     integer          NOT NULL CHECK >= 0
updated_at     timestamptz      NOT NULL

PK (guild_id, tournament_id, user_id)
UQ (guild_id, tournament_id, queue_position)
partial UQ (guild_id, tournament_id) WHERE state='ACTIVE'
FK (guild_id, tournament_id) -> auction_tournaments (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
CHECK ((snapshot_origin IN ('CAPTURED','INFERRED')
        AND declared_lane IS NOT NULL AND mmr_at_start IS NOT NULL)
    OR (snapshot_origin='UNAVAILABLE' AND (declared_lane IS NULL OR mmr_at_start IS NULL)))
```

captain은 candidate queue에 넣지 않는다. `RESTORE_UNSOLD`는 현재 active lot이 없을 때 기존
UNSOLD lot을 보존하고 같은 candidate의 새 ACTIVE lot을 만들어 즉시 current target으로
복원한다. queue 뒤로 보내는 command가 아니다.

### `auction_lots`

```text
guild_id       text        NOT NULL
tournament_id  uuid        NOT NULL
id             uuid        NOT NULL
draw_sequence  bigint      NOT NULL CHECK > 0
target_user_id text        NOT NULL
status         text        NOT NULL CHECK ACTIVE|AWARDED|UNSOLD|ASSIGNED|REVERTED
opened_by_type text        NOT NULL CHECK USER|BOT|SYSTEM|MIGRATION
opened_by_id   text        NOT NULL
opened_at      timestamptz NOT NULL
closed_by_type text        NULL CHECK USER|BOT|SYSTEM|MIGRATION
closed_by_id   text        NULL
closed_at      timestamptz NULL

PK (guild_id, tournament_id, id)
UQ (guild_id, tournament_id, draw_sequence)
UQ (guild_id, tournament_id, id, target_user_id)
FK (guild_id, tournament_id) -> auction_tournaments (guild_id, id) RESTRICT
FK (guild_id, tournament_id, target_user_id)
  -> auction_bid_candidates (guild_id, tournament_id, user_id) RESTRICT
partial UQ (guild_id, tournament_id) WHERE status='ACTIVE'
CHECK ((status='ACTIVE' AND closed_by_type IS NULL AND closed_by_id IS NULL AND closed_at IS NULL)
    OR (status<>'ACTIVE' AND closed_by_type IS NOT NULL AND closed_by_id IS NOT NULL
        AND closed_at IS NOT NULL))
```

### `auction_actions`

```text
guild_id          text        NOT NULL
tournament_id     uuid        NOT NULL
id                uuid        NOT NULL
sequence          bigint      NOT NULL CHECK > 0
kind              text        NOT NULL CHECK
                  DRAW_NEXT|MARK_UNSOLD|AWARD_BID|ASSIGN_CURRENT|
                  ASSIGN_UNSOLD|RESTORE_UNSOLD|REVERT_LAST
target_user_id    text        NULL
team_id           uuid        NULL
lot_id            uuid        NULL
points            integer     NULL CHECK >= 0
reverses_action_id uuid       NULL
reversed_action_kind text     NULL CHECK
                             DRAW_NEXT|MARK_UNSOLD|AWARD_BID|ASSIGN_CURRENT|
                             ASSIGN_UNSOLD|RESTORE_UNSOLD
actor_type        text        NOT NULL CHECK USER|BOT|SYSTEM|MIGRATION
actor_id          text        NOT NULL
created_at        timestamptz NOT NULL

PK (guild_id, tournament_id, id)
UQ (guild_id, tournament_id, sequence)
UQ (guild_id, tournament_id, id, kind)
UQ (guild_id, tournament_id, id, team_id, kind)
UQ (guild_id, tournament_id, id, team_id, target_user_id, kind)
partial UQ (guild_id, tournament_id, reverses_action_id)
  WHERE reverses_action_id IS NOT NULL
FK (guild_id, tournament_id) -> auction_tournaments (guild_id, id) RESTRICT
FK (guild_id, tournament_id, target_user_id) -> auction_bid_candidates (...) RESTRICT
FK (guild_id, tournament_id, team_id) -> auction_teams (...) RESTRICT
FK (guild_id, tournament_id, lot_id, target_user_id)
  -> auction_lots (guild_id, tournament_id, id, target_user_id) RESTRICT
FK (guild_id, tournament_id, reverses_action_id, reversed_action_kind)
  -> auction_actions (guild_id, tournament_id, id, kind) RESTRICT
CHECK ((kind='REVERT_LAST' AND reverses_action_id IS NOT NULL AND
        reversed_action_kind IS NOT NULL) OR
       (kind<>'REVERT_LAST' AND reverses_action_id IS NULL AND
        reversed_action_kind IS NULL))
CHECK (reverses_action_id IS NULL OR reverses_action_id<>id)
CHECK ((kind='REVERT_LAST' AND points IS NULL AND
        (lot_id IS NULL OR target_user_id IS NOT NULL)) OR
       (target_user_id IS NOT NULL AND lot_id IS NOT NULL AND
        ((kind IN ('DRAW_NEXT','MARK_UNSOLD','RESTORE_UNSOLD') AND
          team_id IS NULL AND points IS NULL) OR
         (kind IN ('ASSIGN_CURRENT','ASSIGN_UNSOLD') AND
          team_id IS NOT NULL AND points IS NULL) OR
         (kind='AWARD_BID' AND team_id IS NOT NULL AND points IS NOT NULL))))
```

`auction_actions` 생성 뒤 같은 migration에서 다음 FK를 `ALTER TABLE`로 추가한다.

```text
FK auction_team_members
  (guild_id, tournament_id, acquisition_action_id, team_id, user_id, acquisition_action_kind)
  -> auction_actions
     (guild_id, tournament_id, id, team_id, target_user_id, kind) RESTRICT
```

입찰 command history는 append-only다. `REVERT_LAST`는 원본 action을 수정/삭제하지 않고 새
action이 원본을 참조한다. 현재 상태는 normalized candidate/lot/team/point row에서 읽고 action
ledger는 감사와 정확한 최근 command 판정에 사용한다.

`REVERT_LAST.reversed_action_kind`는 원본 action의 kind를 함께 보존한다. typed composite FK와
self-reference CHECK로 다른 `REVERT_LAST`나 자기 자신을 원본으로 연결할 수 없게 한다.

`AWARD_BID` action이 lot, target, team, points, actor, time을 모두 가진 canonical 낙찰 원장이다.
같은 사실을 반복하는 `auction_bids` table과 frontend에 없는 bid intent table은 만들지 않는다.
`AWARD_BID`는 lot/target/team/points가 모두 필수이며 action을 먼저 insert한 뒤 point ledger와
team member가 이를 참조한다. `REVERT_LAST`의 `reverses_action_id`는 원본 action이고,
current target을 복원해 새 lot을 만드는 경우 `lot_id`는 그 새 lot을 가리킨다.

`REVERT_LAST`가 `MARK_UNSOLD`, `AWARD_BID`, `ASSIGN_CURRENT`를 되돌리면 기존 lot을
`REVERTED`로 닫고 같은 candidate의 새 ACTIVE lot을 만든다. frontend처럼 선수가 즉시 current
target으로 돌아오므로 다음 낙찰은 새 lot ID에 기록한다. `ASSIGN_UNSOLD`를 되돌릴 때는 선수를
UNSOLD로 돌릴 뿐 active lot을 만들지 않는다.

### `auction_point_ledger`

```text
guild_id          text        NOT NULL
tournament_id     uuid        NOT NULL
id                uuid        NOT NULL
team_id           uuid        NOT NULL
point_revision    bigint      NOT NULL CHECK >= 0
reason            text        NOT NULL CHECK INITIAL|BID_DEBIT|REVERSAL
source_action_id  uuid        NULL
source_action_kind text       NULL CHECK AWARD_BID|REVERT_LAST
points_before     integer     NOT NULL CHECK >= 0
points_after      integer     NOT NULL CHECK >= 0
delta             integer     NOT NULL
reverses_entry_id uuid        NULL
reversed_reason   text        NULL CHECK BID_DEBIT
actor_type        text        NOT NULL CHECK USER|BOT|SYSTEM|MIGRATION
actor_id          text        NOT NULL
created_at        timestamptz NOT NULL

PK (guild_id, tournament_id, id)
UQ (guild_id, tournament_id, id, team_id)
UQ (guild_id, tournament_id, id, team_id, reason)
UQ (guild_id, tournament_id, team_id, point_revision)
partial UQ (guild_id, tournament_id, team_id) WHERE reason='INITIAL'
partial UQ (guild_id, tournament_id, source_action_id, team_id)
  WHERE source_action_id IS NOT NULL
partial UQ (guild_id, tournament_id, reverses_entry_id)
  WHERE reverses_entry_id IS NOT NULL
FK (guild_id, tournament_id, team_id) -> auction_teams (...) RESTRICT
FK (guild_id, tournament_id, source_action_id, team_id, source_action_kind)
  -> auction_actions (guild_id, tournament_id, id, team_id, kind) RESTRICT
FK (guild_id, tournament_id, reverses_entry_id, team_id, reversed_reason)
  -> auction_point_ledger (guild_id, tournament_id, id, team_id, reason) RESTRICT
CHECK ((reason='INITIAL' AND source_action_id IS NULL AND source_action_kind IS NULL AND
        point_revision=0)
    OR (reason='BID_DEBIT' AND source_action_id IS NOT NULL AND
        source_action_kind='AWARD_BID' AND point_revision>0)
    OR (reason='REVERSAL' AND source_action_id IS NOT NULL AND
        source_action_kind='REVERT_LAST' AND point_revision>0))
CHECK ((reason='REVERSAL' AND reverses_entry_id IS NOT NULL AND reversed_reason='BID_DEBIT') OR
       (reason<>'REVERSAL' AND reverses_entry_id IS NULL AND reversed_reason IS NULL))
CHECK (reverses_entry_id IS NULL OR reverses_entry_id<>id)
CHECK (points_after = points_before + delta)
```

`points_after = points_before + delta`와 summary 일치는 transaction test로 검증한다. runtime
role에는 이 ledger의 UPDATE/DELETE 권한을 주지 않는다.
INITIAL row는 `point_revision=0`, `points_before=0`,
`delta=points_after=auction_teams.initial_points`이며 team summary도 point_revision 0이다.
BID_DEBIT/REVERSAL은 locked team의 point_revision+1을 ledger와 summary에 함께 기록하고 정확한
tail은 timestamp가 아니라 point_revision으로 판정한다.
REVERSAL의 `reversed_reason`은 반드시 `BID_DEBIT`이며 typed composite FK로 INITIAL, 다른
REVERSAL, 자기 자신을 취소 대상으로 삼을 수 없다.
REVERSAL은 같은 team의 unreversed `BID_DEBIT` tail만 참조할 수 있으며 application이 team point
row와 ledger tail을 함께 lock해 검증한다.

## 경매 bracket과 match

### `auction_bracket_slots`

```text
guild_id      text        NOT NULL
tournament_id uuid        NOT NULL
slot_key      text        NOT NULL CHECK A_0|A_1|B_0|B_1
team_id       uuid        NULL
updated_by    text        NULL
updated_at    timestamptz NOT NULL

PK (guild_id, tournament_id, slot_key)
partial UQ (guild_id, tournament_id, team_id) WHERE team_id IS NOT NULL
FK (guild_id, tournament_id) -> auction_tournaments (guild_id, id) RESTRICT
FK (guild_id, tournament_id, team_id) -> auction_teams (...) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
```

- TEN_PERSON은 `A_0`, `A_1`만 사용한다.
- TWENTY_PERSON은 네 slot을 모두 사용한다.
- format별 허용 slot과 lock 시 필수 slot 완성은 `LOCK_BRACKET` transaction이 검증한다.
- bracket revision과 lock metadata는 `auction_tournaments`가 소유한다.
- legacy partial IN_GAME처럼 기존 match가 있지만 bracket이 unlocked인 경우 기존 match에 연결된 slot은
  변경/삭제할 수 없다. 나머지 slot만 완성한 뒤 `LOCK_BRACKET`이 team/key 일치를 검증하고 missing
  match만 생성한다.

### `auction_matches`

```text
guild_id         text        NOT NULL
tournament_id    uuid        NOT NULL
id               uuid        NOT NULL
season_id        uuid        NOT NULL
match_key        text        NOT NULL CHECK SEMIFINAL_A|SEMIFINAL_B|FINAL
best_of          smallint    NOT NULL CHECK IN (1,3,5)
hard_fearless    boolean     NOT NULL DEFAULT false
left_team_id     uuid        NULL
right_team_id    uuid        NULL
status           text        NOT NULL CHECK LINEUP_PENDING|READY|IN_PROGRESS|COMPLETED|CANCELLED
winner_team_id   uuid        NULL
revision         bigint      NOT NULL CHECK >= 0
lineups_locked_at timestamptz NULL
lineups_locked_by text       NULL
lineup_lock_invariant_version smallint NULL
started_at       timestamptz NULL
ended_at         timestamptz NULL
updated_by       text        NULL
updated_at       timestamptz NOT NULL
deleted_at       timestamptz NULL

PK (guild_id, tournament_id, id)
partial UQ (guild_id, tournament_id, match_key)
  WHERE status<>'CANCELLED' AND deleted_at IS NULL
UQ (guild_id, id)
UQ (guild_id, season_id, id)
FK (guild_id, season_id, tournament_id)
  -> auction_tournaments (guild_id, season_id, id) RESTRICT
FK (guild_id, tournament_id, left_team_id) -> auction_teams (...) RESTRICT
FK (guild_id, tournament_id, right_team_id) -> auction_teams (...) RESTRICT
FK (guild_id, tournament_id, winner_team_id) -> auction_teams (...) RESTRICT
FK (guild_id, lineups_locked_by) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
CHECK (left_team_id IS NULL OR right_team_id IS NULL OR left_team_id<>right_team_id)
CHECK (winner_team_id IS NULL OR
       (left_team_id IS NOT NULL AND right_team_id IS NOT NULL AND
        (winner_team_id=left_team_id OR winner_team_id=right_team_id)))
CHECK ((status IN ('LINEUP_PENDING','READY','IN_PROGRESS') AND
        winner_team_id IS NULL AND ended_at IS NULL) OR
       (status='COMPLETED' AND winner_team_id IS NOT NULL AND ended_at IS NOT NULL) OR
       (status='CANCELLED' AND winner_team_id IS NULL AND ended_at IS NOT NULL))
```

- TEN_PERSON은 `FINAL` 하나다.
- TWENTY_PERSON은 `SEMIFINAL_A`, `SEMIFINAL_B`, `FINAL`이다.
- match의 `best_of`와 `hard_fearless`는 tournament rules snapshot에서 생성 시 복사된 값이다. source
  recruitment를 나중에 다시 읽어 규칙을 바꾸지 않는다.
- final team은 semifinal 완료 전 NULL일 수 있다.
- semifinal winner로 빈 final의 team이 확정될 때는 각 진출 team의 직전 locked match lineup을
  final의 initial lineup row로 복사하되 lock metadata는 비워 둔다. 운영자가 final별 변경을 검토한
  뒤 다시 lock한다. 직전 lineup이 없거나 final에 이미 편집 row가 있으면 자동 덮어쓰지 않는다.
- score와 `resultEntered`는 confirmed games에서 파생한다.
- winner는 confirmed game 다수가 결정되면 같은 transaction에서 기록한다.
- `auction_matches_lineup_lock_state_check`는 `lineups_locked_at`/`lineups_locked_by`가 함께 존재하거나
  함께 비어 있어야 하며, 신규 `READY`/`IN_PROGRESS`/`COMPLETED` match에는 둘 다 존재하도록 강제한다.
  migration `0018` 뒤 `0019`는 nullable `lineup_lock_invariant_version`을 추가한다. 실제 invariant를
  만족하는 기존 row와 모든 신규 row는 `1`이고, lock actor provenance를 복원할 수 없는 기존 위반 row만
  `NULL` legacy exemption으로 남는다. runtime/API는 이 column에 `NULL`을 쓰지 않는다.
- `NULL` exemption은 기존 row의 unrelated revision/state update를 막지 않기 위한 DB-only compatibility
  marker다. remediation은 실제 lock metadata와 `lineup_lock_invariant_version=1`을 같은 transaction에서
  기록해야 하며, `NULL` row가 0개가 된 뒤에만 별도 migration으로 exemption을 제거하거나 column을
  `NOT NULL`로 강화한다. actor를 추정하거나 `VALIDATE CONSTRAINT`만 실행해서 remediation을 완료했다고
  판단하지 않는다.
- bracket 생성 되돌리기는 기존 match를 CANCELLED로 보존한다. 다시 생성하면 같은 match key의
  새 match ID를 발급한다.

### `auction_match_lineups`

```text
guild_id          text NOT NULL
tournament_id     uuid NOT NULL
match_id          uuid NOT NULL
team_id           uuid NOT NULL
lane              text NOT NULL CHECK canonical lane
team_member_id    uuid NOT NULL
user_id           text NOT NULL

PK (guild_id, tournament_id, match_id, team_id, lane)
UQ (guild_id, tournament_id, match_id, user_id)
FK (guild_id, tournament_id, match_id) -> auction_matches (...) RESTRICT
FK (guild_id, tournament_id, team_id, team_member_id, user_id)
  -> auction_team_members (guild_id, tournament_id, team_id, id, user_id) RESTRICT
```

Lock 시 양 팀 각각 5개 lane, 사용자 중복 없음, active team membership을 검증한다. game이 하나라도
확정된 match의 lineup unlock/교체는 거부한다.

## Game 입력 draft와 확정 원장

일반 series와 auction match가 공통 table을 사용한다. 이 table group은 두 parent가 모두 생성된
뒤 migration한다.

### `game_inputs`

```text
guild_id        text        NOT NULL
id              uuid        NOT NULL
series_id       uuid        NULL
auction_match_id uuid       NULL
set_number      smallint    NOT NULL CHECK 1..5
side_preset     text        NOT NULL CHECK DEFAULT|SWAPPED
winning_side    text        NULL CHECK BLUE|RED
patch_version   text        NULL
status          text        NOT NULL CHECK DRAFT|SUBMITTED|SUPERSEDED
updated_by      text        NULL
updated_at      timestamptz NOT NULL

PK (guild_id, id)
CHECK (num_nonnulls(series_id, auction_match_id) = 1)
UQ (guild_id, id, series_id, set_number, side_preset, winning_side)
UQ (guild_id, id, auction_match_id, set_number, side_preset, winning_side)
FK (guild_id, series_id) -> series (guild_id, id) RESTRICT
FK (guild_id, auction_match_id) -> auction_matches (guild_id, id) RESTRICT
FK (guild_id, updated_by) -> guild_members (guild_id, user_id) RESTRICT
partial UQ (guild_id, series_id, set_number)
  WHERE series_id IS NOT NULL AND status IN ('DRAFT','SUBMITTED')
partial UQ (guild_id, auction_match_id, set_number)
  WHERE auction_match_id IS NOT NULL AND status IN ('DRAFT','SUBMITTED')
```

Frontend 의미:

```text
DEFAULT = 왼쪽/TEAM_1이 BLUE, 오른쪽/TEAM_2가 RED
SWAPPED = 왼쪽/TEAM_1이 RED, 오른쪽/TEAM_2가 BLUE
winner  = winning_side (BLUE 또는 RED)
```

승리 team은 `side_preset + winning_side`로 계산한다. input 완료 전에는 winner가 NULL일 수 있고
submit 시에는 반드시 값이 있어야 한다.

### `game_input_picks`

```text
guild_id     text NOT NULL
game_input_id uuid NOT NULL
user_id      text NOT NULL
champion_id  text NOT NULL

PK (guild_id, game_input_id, user_id)
FK (guild_id, game_input_id) -> game_inputs (guild_id, id) CASCADE
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
```

### `game_input_bans`

```text
guild_id     text     NOT NULL
game_input_id uuid    NOT NULL
side         text     NOT NULL CHECK BLUE|RED
slot         smallint NOT NULL CHECK 1..5
champion_id  text     NOT NULL

PK (guild_id, game_input_id, side, slot)
FK (guild_id, game_input_id) -> game_inputs (guild_id, id) CASCADE
```

Pick/ban은 optional이다. UI에 5개 slot이 있어도 입력된 row만 저장하며 정확히 5개를 강제하지
않는다.

### `games`

```text
guild_id         text        NOT NULL
id               uuid        NOT NULL
season_id        uuid        NOT NULL
series_id        uuid        NULL
auction_match_id uuid        NULL
source_input_id  uuid        NOT NULL
game_number      smallint    NOT NULL CHECK 1..5
side_preset      text        NOT NULL CHECK DEFAULT|SWAPPED
winning_side     text        NOT NULL CHECK BLUE|RED
duration_seconds integer     NULL CHECK > 0
patch_version    text        NULL
riot_match_id    text        NULL
status           text        NOT NULL CHECK CONFIRMED|VOIDED
mmr_status       text        NOT NULL CHECK APPLIED|LEGACY_NOT_APPLIED|REVERSED
played_at        timestamptz NOT NULL
confirmed_by     text        NOT NULL
confirmed_by_type text       NOT NULL CHECK USER|BOT|MIGRATION
confirmed_at     timestamptz NOT NULL
voided_by        text        NULL
voided_by_type   text        NULL CHECK USER|BOT|SYSTEM
voided_at        timestamptz NULL
void_reason      text        NULL

PK (guild_id, id)
UQ (guild_id, source_input_id)
UQ (guild_id, season_id, id)
partial UQ (guild_id, riot_match_id) WHERE riot_match_id IS NOT NULL
CHECK (num_nonnulls(series_id, auction_match_id) = 1)
partial UQ (guild_id, series_id, game_number)
  WHERE series_id IS NOT NULL AND status='CONFIRMED'
partial UQ (guild_id, auction_match_id, game_number)
  WHERE auction_match_id IS NOT NULL AND status='CONFIRMED'
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, season_id, series_id) -> series (guild_id, season_id, id) RESTRICT
FK (guild_id, season_id, auction_match_id)
  -> auction_matches (guild_id, season_id, id) RESTRICT
FK (guild_id, source_input_id, series_id, game_number, side_preset, winning_side)
  -> game_inputs (guild_id, id, series_id, set_number, side_preset, winning_side) RESTRICT
FK (guild_id, source_input_id, auction_match_id, game_number, side_preset, winning_side)
  -> game_inputs (guild_id, id, auction_match_id, set_number, side_preset, winning_side) RESTRICT
CHECK ((status='VOIDED' AND voided_by IS NOT NULL AND voided_by_type IS NOT NULL AND
        voided_at IS NOT NULL AND void_reason IS NOT NULL) OR
       (status='CONFIRMED' AND voided_by IS NULL AND voided_by_type IS NULL AND
        voided_at IS NULL AND void_reason IS NULL))
CHECK ((status='CONFIRMED' AND mmr_status IN ('APPLIED','LEGACY_NOT_APPLIED')) OR
       (status='VOIDED' AND mmr_status IN ('REVERSED','LEGACY_NOT_APPLIED')))
```

Rules:

- 신규 confirmed game은 `mmr_status=APPLIED`다.
- v3의 과거 경매 game만 `LEGACY_NOT_APPLIED`를 허용한다.
- Custom match 입력에는 duration field가 없으므로 `duration_seconds`는 nullable이다. Legacy/Riot link로
  신뢰 가능한 값이 있을 때만 기록하고 read DTO도 null을 보존한다.
- game revert는 row를 삭제하지 않고 `status=VOIDED`, `mmr_status=REVERSED`로 바꾼다.
- void 시 원본 input은 `SUPERSEDED`가 되어 같은 game number를 새 input으로 재입력할 수 있다.
- void actor/type/time/reason 네 필드는 모두 NULL이거나 모두 NOT NULL이어야 한다.
- game과 MMR ledger FK에는 `ON DELETE CASCADE`를 사용하지 않는다.

### `game_participants`

```text
guild_id    text NOT NULL
game_id     uuid NOT NULL
user_id     text NOT NULL
team        text NOT NULL CHECK TEAM_1|TEAM_2
lane        text NOT NULL CHECK canonical lane
champion_id text NULL

PK (guild_id, game_id, user_id)
UQ (guild_id, game_id, team, lane)
FK (guild_id, game_id) -> games (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
```

실제 side는 `team + games.side_preset`에서 파생한다. 일반 game은 frozen series roster, 경매
game은 locked lineup에 있는 사용자만 허용한다.

### `game_participant_stats`

```text
guild_id       text    NOT NULL
game_id        uuid    NOT NULL
user_id        text    NOT NULL
kills          integer NULL CHECK >= 0
deaths         integer NULL CHECK >= 0
assists        integer NULL CHECK >= 0
cs             integer NULL CHECK >= 0

PK (guild_id, game_id, user_id)
FK (guild_id, game_id, user_id) -> game_participants (...) RESTRICT
```

신규 Custom match 입력에서 필수가 아니다. v3의 KDA/CS를 손실 없이 옮기거나 추후 신뢰할 수
있는 source가 생길 때만 row를 쓴다.

### `game_bans`

```text
guild_id   text     NOT NULL
game_id    uuid     NOT NULL
side       text     NOT NULL CHECK BLUE|RED
slot       smallint NOT NULL CHECK 1..5
champion_id text    NOT NULL

PK (guild_id, game_id, side, slot)
FK (guild_id, game_id) -> games (guild_id, id) RESTRICT
```

Game submit은 `game_inputs`, picks, bans를 확정 game tables로 복사하고 input을 `SUBMITTED`로
바꾼다. 확정 원장은 이후 pick/ban 수정으로 덮어쓰지 않는다.

## Guild MMR

### `user_lane_mmr`

```text
guild_id     text             NOT NULL
season_id    uuid             NOT NULL
user_id      text             NOT NULL
lane         text             NOT NULL CHECK canonical lane
mmr          double precision NOT NULL DEFAULT 1500
games_played integer          NOT NULL DEFAULT 0 CHECK >= 0
wins         integer          NOT NULL DEFAULT 0 CHECK >= 0
revision     bigint           NOT NULL CHECK >= 0
updated_at   timestamptz      NOT NULL

PK (guild_id, season_id, user_id, lane)
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
CHECK wins <= games_played
```

`mmr`은 조회와 row lock을 위한 현재 summary다. source of change history는 ledger이며 두 table은
항상 같은 transaction에서 갱신한다. NaN과 positive/negative infinity는 CHECK로 거부한다.
Records와 admin read projection은 아직 row가 없는 canonical lane도 `mmr=1500`, `games_played=0`,
`wins=0`으로 반환하되 조회만으로 row를 생성하지 않는다. Records DTO에는 의미가 동결되지 않은 별도
`change` field를 두지 않는다.

존재하지 않는 row는 `SELECT FOR UPDATE`로 잠글 수 없으므로 모든 MMR mutation은 공통
`ensure_and_lock_mmr_rows` 절차를 사용한다.

```text
1. 필요한 (guild_id, season_id, user_id, lane)를 user_id/lane 순으로 unique sort
2. 같은 순서로 row마다
   INSERT (mmr=1500, games_played=0, wins=0, revision=0, updated_at=now())
   ON CONFLICT (guild_id, season_id, user_id, lane) DO NOTHING
3. 같은 순서로 SELECT ... FOR UPDATE하고 요청한 key가 모두 반환됐는지 검증
```

동시 첫 생성은 unique index에서 한 transaction을 기다린 뒤 conflict path로 합류한다. 여러 key를
항상 같은 순서로 insert/lock해 교차 game의 deadlock 가능성을 줄인다. 이 helper는 series/auction
snapshot, game submit/revert, admin manual adjustment에 공통 사용한다. deadlock SQLSTATE는
idempotency key를 유지한 채 bounded full-transaction retry하고 부분 결과를 반환하지 않는다.

### `mmr_ledger_entries`

```text
guild_id          text             NOT NULL
id                uuid             NOT NULL
season_id         uuid             NOT NULL
user_id           text             NOT NULL
lane              text             NOT NULL CHECK canonical lane
lane_revision     bigint           NOT NULL CHECK > 0
source_type       text             NOT NULL CHECK
                  GAME_RESULT|GAME_REVERSAL|MANUAL_ADJUSTMENT|
                  MANUAL_REVERSAL|MIGRATION_SEED
source_id         uuid             NOT NULL
game_id           uuid             NULL
opponent_user_id  text             NULL
mmr_before        double precision NOT NULL
mmr_after         double precision NOT NULL
delta             double precision NOT NULL
games_played_delta smallint        NOT NULL
wins_delta        smallint         NOT NULL
reverses_entry_id uuid             NULL
reversed_source_type text          NULL CHECK GAME_RESULT|MANUAL_ADJUSTMENT
actor_type        text             NOT NULL CHECK USER|BOT|SYSTEM|MIGRATION
actor_id          text             NOT NULL
note              text             NULL
request_id        text             NULL
created_at        timestamptz      NOT NULL

PK (guild_id, id)
UQ (guild_id, id, season_id, user_id, lane)
UQ (guild_id, id, season_id, user_id, lane, source_type)
UQ (guild_id, season_id, user_id, lane, lane_revision)
FK (guild_id, season_id) -> seasons (guild_id, id) RESTRICT
FK (guild_id, user_id) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, season_id, game_id) -> games (guild_id, season_id, id) RESTRICT
FK (guild_id, opponent_user_id) -> guild_members (guild_id, user_id) RESTRICT
FK (guild_id, game_id, user_id)
  -> game_participants (guild_id, game_id, user_id) RESTRICT
FK (guild_id, game_id, opponent_user_id)
  -> game_participants (guild_id, game_id, user_id) RESTRICT
FK (guild_id, reverses_entry_id, season_id, user_id, lane, reversed_source_type)
  -> mmr_ledger_entries
     (guild_id, id, season_id, user_id, lane, source_type) RESTRICT
UQ (guild_id, source_type, source_id, user_id, lane)
partial UQ (guild_id, game_id, user_id, lane)
  WHERE source_type='GAME_RESULT' AND game_id IS NOT NULL
partial UQ (guild_id, reverses_entry_id) WHERE reverses_entry_id IS NOT NULL
CHECK (source_type<>'MANUAL_ADJUSTMENT' OR delta<>0)
CHECK ((source_type IN ('GAME_REVERSAL','MANUAL_REVERSAL')) =
       (reverses_entry_id IS NOT NULL))
CHECK ((source_type='GAME_REVERSAL' AND reversed_source_type='GAME_RESULT') OR
       (source_type='MANUAL_REVERSAL' AND reversed_source_type='MANUAL_ADJUSTMENT') OR
       (source_type NOT IN ('GAME_REVERSAL','MANUAL_REVERSAL') AND
        reversed_source_type IS NULL))
CHECK ((source_type IN ('GAME_RESULT','GAME_REVERSAL')) = (game_id IS NOT NULL))
CHECK (source_type NOT IN ('GAME_RESULT','GAME_REVERSAL') OR user_id<>opponent_user_id)
CHECK (source_type<>'MANUAL_ADJUSTMENT' OR note IS NOT NULL)
CHECK ((source_type='GAME_RESULT' AND opponent_user_id IS NOT NULL AND
        games_played_delta=1 AND wins_delta IN (0,1)) OR
       (source_type='GAME_REVERSAL' AND opponent_user_id IS NOT NULL AND
        games_played_delta=-1 AND wins_delta IN (-1,0)) OR
       (source_type IN ('MANUAL_ADJUSTMENT','MANUAL_REVERSAL') AND
        opponent_user_id IS NULL AND games_played_delta=0 AND wins_delta=0) OR
       (source_type='MIGRATION_SEED' AND opponent_user_id IS NULL))
```

Rules:

- `GAME_RESULT`: game_id와 opponent가 필수, game/win count delta는 1/0 또는 1/1이다.
- `GAME_REVERSAL`: 원본 game entry를 하나씩 참조하고 count delta도 반대로 적용한다.
- 두 game source type의 user와 opponent는 모두 같은 game의 참가자여야 한다. 서로 다른 사용자,
  같은 lane, 반대 team인지는 game submit/reversal transaction에서 검증한다.
- `MANUAL_ADJUSTMENT`: game_id/opponent는 NULL, operator note는 필수다.
- `MANUAL_ADJUSTMENT`: delta는 0일 수 없다.
- `MANUAL_REVERSAL`: 같은 season/user/lane의 unreversed MANUAL_ADJUSTMENT tail을 참조한다.
- 두 reversal은 composite FK가 보장한 원본 type 외에도 같은 game/source 의미와 정확히 반대인
  MMR/count delta, 뒤집힌 before/after를 transaction에서 검증한다.
- `MIGRATION_SEED`: legacy summary reconciliation에만 쓴다.
- append 시 `lane_revision = locked user_lane_mmr.revision + 1`로 기록하고 summary revision도 같은
  값으로 갱신한다. row의 정확한 tail은 timestamp가 아니라 lane_revision으로 판정한다.
- `mmr_after = mmr_before + delta`는 Rust domain calculation 결과를 그대로 저장한다. float
  equality를 임의 rounding한 SQL CHECK로 강제하지 않는다.
- runtime role에는 ledger UPDATE/DELETE 권한을 주지 않는다.

MMR revert는 마지막 confirmed game만 대상으로 한다. 모든 참가자 lane의 현재 summary가 대상 game의
`mmr_after`/count를 그대로 가리키고, 이후 ledger가 있다면 이미 `GAME_REVERSAL`로 상쇄된 후속 game 결과만
허용한다. 따라서 여러 confirmed set을 마지막 set부터 역순으로 void하는 series lifecycle cascade는
가능하지만, 아직 상쇄되지 않은 후속 game, manual adjustment/reversal, migration seed가 하나라도 있으면
`series_game_reversal_not_safe`로 거부한다. 과거 중간 ELO를 단순 반대 delta로 보정하는 기능은 MVP에
만들지 않는다.

## Safety와 전달 원장

### `revert_checkpoints`

```text
guild_id       text        NOT NULL
id             uuid        NOT NULL
target_type    text        NOT NULL
target_id      text        NOT NULL
target_revision bigint     NOT NULL CHECK >= 0
action         text        NOT NULL
schema_version integer     NOT NULL CHECK > 0
restore_payload jsonb      NOT NULL
status          text       NOT NULL CHECK ACTIVE|REVERTED|EXPIRED
created_by_type text       NOT NULL CHECK USER|BOT|SYSTEM
created_by      text       NOT NULL
created_at      timestamptz NOT NULL
reverted_by_type text      NULL
reverted_by     text       NULL
reverted_at     timestamptz NULL

PK (guild_id, id)
FK (guild_id) -> guilds (guild_id) RESTRICT
CHECK ((status='REVERTED' AND reverted_by_type IS NOT NULL AND
        reverted_by IS NOT NULL AND reverted_at IS NOT NULL) OR
       (status IN ('ACTIVE','EXPIRED') AND reverted_by_type IS NULL AND
        reverted_by IS NULL AND reverted_at IS NULL))
```

Checkpoint는 허용된 고정 operation만 만들며 arbitrary database snapshot이나 범용 undo/redo로
사용하지 않는다. MMR/game reversal은 JSON restore가 아니라 명시적 void와 ledger row로
수행한다.

### `admin_audit_log`

```text
guild_id        text        NOT NULL
id              uuid        NOT NULL
principal_type  text        NOT NULL CHECK USER|BOT|SYSTEM|MIGRATION
principal_id    text        NOT NULL
action          text        NOT NULL
target_type     text        NULL
target_id       text        NULL
checkpoint_id   uuid        NULL
request_id      text        NULL
before_summary  jsonb       NULL
after_summary   jsonb       NULL
result          text        NOT NULL CHECK APPLIED|REVERTED
note            text        NULL
created_at      timestamptz NOT NULL

PK (guild_id, id)
FK (guild_id, checkpoint_id) -> revert_checkpoints (guild_id, id) RESTRICT
FK (guild_id) -> guilds (guild_id) RESTRICT
```

Append-only이며 runtime role에는 UPDATE/DELETE 권한을 주지 않는다. access token, cookie,
Riot API key, 전체 draft는 payload에 넣지 않는다.

### `idempotency_records`

```text
id                uuid        PK
guild_id          text        NULL FK -> guilds.guild_id RESTRICT
scope_key         text        NOT NULL
principal_type    text        NOT NULL CHECK USER|BOT|SERVICE
principal_id      text        NOT NULL
method            text        NOT NULL
route_template    text        NOT NULL
idempotency_key   text        NOT NULL
request_hash      bytea       NOT NULL
state             text        NOT NULL CHECK IN_PROGRESS|COMPLETED
status_code       smallint    NULL CHECK 100..599
response_body     jsonb       NULL
created_at        timestamptz NOT NULL
completed_at      timestamptz NULL
expires_at        timestamptz NOT NULL

CHECK (scope_key = COALESCE(guild_id, 'GLOBAL'))
CHECK ((state='IN_PROGRESS' AND status_code IS NULL AND response_body IS NULL AND completed_at IS NULL)
    OR (state='COMPLETED' AND status_code IS NOT NULL AND completed_at IS NOT NULL))
UQ (scope_key, principal_type, principal_id, method, route_template, idempotency_key)
```

- 같은 key와 같은 request hash는 완료된 response를 replay한다.
- 같은 key와 다른 hash는 `409 idempotency_conflict`다.
- 같은 command identity는 transaction-scoped advisory lock을 먼저 `try`한다. 선행 transaction이
  진행 중이면 unique index wait나 `lock_timeout` 오류를 만들지 않고 `in_progress`를 반환한다.
- advisory key는 namespace와 unique command column 전체를 구조화해 hash한다. 구분자를 이어 붙인
  문자열 key는 사용하지 않는다.
- `expectedRevision`도 request hash에 포함한다.
- idempotency row, domain mutation, audit, outbox는 같은 transaction에 commit한다.
- response에는 필요한 최소 body만 저장하며 기본 retention은 24시간이다.

### `outbox_events`

```text
id                 uuid        PK
guild_id           text        NOT NULL FK -> guilds.guild_id RESTRICT
topic              text        NOT NULL
aggregate_type     text        NOT NULL
aggregate_id       text        NOT NULL
aggregate_revision bigint      NOT NULL CHECK >= 0
event_type         text        NOT NULL
payload            jsonb       NOT NULL
occurred_at        timestamptz NOT NULL
available_at       timestamptz NOT NULL
attempts           integer     NOT NULL DEFAULT 0 CHECK >= 0
lease_owner        text        NULL
lease_token        uuid        NULL
lease_until        timestamptz NULL
published_at       timestamptz NULL
last_error         text        NULL

CHECK (num_nonnulls(lease_owner, lease_token, lease_until) IN (0,3))
CHECK (published_at IS NULL OR lease_owner IS NULL)
```

Indexes:

- `(available_at, id) WHERE published_at IS NULL`
- `(lease_until) WHERE published_at IS NULL AND lease_until IS NOT NULL`
- `(guild_id, aggregate_type, aggregate_id, aggregate_revision)`

Relay는 DB transaction을 잡은 채 Valkey network I/O를 하지 않는다.

```text
1. 짧은 transaction에서 아래 predicate로 SKIP LOCKED row 선택
   published_at IS NULL
   AND available_at <= now()
   AND (lease_until IS NULL OR lease_until < now())
2. claim마다 새 UUID lease_token을 만들고 lease_owner/token/until 갱신 후 commit
3. transaction 밖에서 Valkey publish
4. 성공 ack와 실패 nack 모두
   WHERE id=? AND lease_owner=? AND lease_token=? AND published_at IS NULL
   조건으로 갱신하고 row count 1을 확인
5. ack는 published_at을 기록하고 lease를 비운다. nack은 application absolute timestamp가 아니라
   retry delay를 받아 `available_at=clock_timestamp()+delay`와 attempts/last_error를 기록하고 lease를
   비운다
```

전달은 at-least-once다. client는 event ID와 revision으로 duplicate/out-of-order event를
허용하고 HTTP snapshot을 다시 조회한다. published row는 기본 7일 보존 후 batch cleanup한다.

### `bot_notification_outbox`

Discord message 전송은 WebSocket invalidate와 목적, retry, 결과가 다르므로 별도 outbox를 쓴다.

```text
id                  uuid        PK
guild_id            text        NOT NULL FK -> guilds.guild_id RESTRICT
deduplication_key   text        NOT NULL
discord_nonce       text        NOT NULL CHECK (char_length(discord_nonce) BETWEEN 1 AND 25)
channel_id          text        NOT NULL
notification_type   text        NOT NULL
payload             jsonb       NOT NULL
created_at          timestamptz NOT NULL
available_at         timestamptz NOT NULL
attempts             integer     NOT NULL DEFAULT 0 CHECK >= 0
first_attempt_at     timestamptz NULL
lease_owner          text        NULL
lease_token          uuid        NULL
lease_until          timestamptz NULL
discord_message_id   text        NULL
delivered_at         timestamptz NULL
failed_at            timestamptz NULL
failure_code         text        NULL
last_error           text        NULL

UQ (guild_id, deduplication_key)
UQ (discord_nonce)
CHECK (num_nonnulls(lease_owner, lease_token, lease_until) IN (0,3))
CHECK ((attempts=0 AND first_attempt_at IS NULL)
    OR (attempts>0 AND first_attempt_at IS NOT NULL))
CHECK (NOT (delivered_at IS NOT NULL AND failed_at IS NOT NULL))
CHECK ((failed_at IS NULL) = (failure_code IS NULL))
CHECK (delivered_at IS NULL OR discord_message_id IS NOT NULL)
CHECK (failed_at IS NULL OR (discord_message_id IS NULL AND last_error IS NOT NULL))
CHECK ((delivered_at IS NULL AND failed_at IS NULL) OR lease_owner IS NULL)
partial index (available_at, id) WHERE delivered_at IS NULL AND failed_at IS NULL
partial index (lease_until)
  WHERE delivered_at IS NULL AND failed_at IS NULL AND lease_until IS NOT NULL
```

Notification worker도 outbox와 같은 available/expired-lease claim, claim별 lease token,
owner+token 조건부 ack/nack을 사용한다. `attempts`는 전송 claim 횟수다. claim transaction은
`attempts < 8`인 pending row만 선택하고 `attempts=attempts+1`,
`first_attempt_at=COALESCE(first_attempt_at, now())`, lease owner/token/until을 함께 기록한 뒤
commit한다. nack에서는 attempts를 다시 증가시키지 않고 available_at/backoff/last_error와 lease만
갱신한다. attempts 8인 claim이 실패하면 `MAX_ATTEMPTS`로 terminal 처리하며, 8번째 claim 뒤 worker가
종료되어 lease가 만료된 row도 reaper가 `MAX_ATTEMPTS`로 terminal 처리해 영구 pending을 막는다.

`discord_nonce`는 outbox UUID의 22자 base64url 표현처럼 재시도해도 같은 값으로 만들고
create-message 요청에 nonce uniqueness 강제를 사용한다. 응답 유실은 같은 nonce로 보수적인 Discord
dedup window 안에서만 재시도하며 끝까지 확인되지 않으면 중복 전송 대신 `DELIVERY_UNKNOWN`
terminal failure로 보내 운영자가 reconcile한다. known message ID edit는 같은 payload로 재실행한다.

Discord 429는 `retry_after`를 따르고 network timeout/5xx는 capped exponential
backoff+jitter로 재시도한다. 인증/권한/잘못된 channel·payload 등 non-retryable 4xx는 즉시 terminal
failure로, retryable 오류도 기본 8회 실패하면 `MAX_ATTEMPTS`로 전이한다. terminal update도
`WHERE id=? AND lease_owner=? AND lease_token=? AND delivered_at IS NULL AND failed_at IS NULL`을 사용해
`failed_at/failure_code/last_error`를 기록하고 lease를 비운다. terminal row는 worker pending count에서
제외하고 admin 상태/metric에서 별도 노출한다.

### `bot_instances`

```text
instance_id       text        PK
version           text        NOT NULL
commit_sha        text        NULL
started_at        timestamptz NOT NULL
last_heartbeat_at timestamptz NOT NULL
updated_at        timestamptz NOT NULL
```

`/internal/heartbeat`가 instance별 row를 upsert한다. `/api/admin/status`의
normal/delayed/down label은 `last_heartbeat_at`과 현재 시각에서 파생하며 status 문자열을 저장하지
않는다. `<=90초`는 normal, `>90초~<=5분`은 delayed, 미수신 또는 `>5분`은 down이다. Heartbeat는
성공한 Bot 생존 신호만 기록하므로 stale 상태라도 UI의 `lastHealthyAt`은 마지막
`last_heartbeat_at`을 보존한다. Main/dev는 database 자체가 분리되므로 environment column으로 논리
격리하지 않는다.

## Revision owner

Frontend 협업 계약과 DB revision owner는 다음과 같다.

| 화면/작업                   | Revision owner                           |
| --------------------------- | ---------------------------------------- |
| 일반 모집 엔트리            | `recruitment_entry_drafts.revision`      |
| 경매 팀장/시작 point 준비   | `auction_setup_drafts.revision`          |
| 일반 pick/ban과 game 결과   | `series.revision`                        |
| 경매 draw/낙찰/배치/bracket | `auction_tournaments.revision`           |
| 경매 match lineup/result    | `auction_matches.revision`               |
| admin member/MMR            | `guild_admin_state[MEMBERS].revision`    |
| admin checkpoint/revert     | `guild_admin_state[OPERATIONS].revision` |
| admin settings              | `guild_config.revision`                  |

한 command가 두 aggregate projection을 바꾸면 둘 다 lock하고 revision을 각각 한 번 올린다. 예를
들어 경매 semifinal 완료로 final team이 결정되면 semifinal match와 tournament revision을
갱신하고 필요한 final match revision도 갱신한다.

## 저장하지 않는 projection

다음 값은 원장 중복이므로 table/column으로 저장하지 않는다.

- operation queue의 한국어 status/detail, href, priority
- series와 match의 score, win/loss count, `resultEntered`
- bracket의 placement label과 우승 문구
- leaderboard rank, win rate, main champion, roles played, `isMe`
- records의 series/tournament 총 MMR 증감, champion 승패, 상대 이름
- synergy와 head-to-head summary
- guild member count와 현재 사용자의 role label
- `lastSavedAt`, 접속 중 editor 목록, presence
- 모든 상대 시간과 한국어 enum label

조회는 confirmed/non-voided games와 append-only ledger를 기준으로 계산한다. 실제 query가
비싸다는 측정 결과가 나온 뒤에만 별도 read model을 추가한다.

Leaderboard는 ACTIVE season만 조회한다. ACTIVE member의 `user_lane_mmr` 중 `games_played >= 1`인
lane만 projection하고, player의 `compositeMmr`는 별도 column 없이
`SUM(mmr * games_played) / SUM(games_played)`로 계산한다. `mainRole`은 실제 플레이한
`guild_members.main_lane`을 우선하고, 없으면 games 내림차순, MMR 내림차순, canonical lane 순으로
결정한다. lane별 대표 champion은 confirmed game 중 취소/삭제되지 않은 parent 아래의 참가 기록을
champion별로 집계해 games 내림차순, champion ID 오름차순으로 고른다. Main Riot ID와 profile icon은
`user_privacy_settings.show_riot_ids=true`일 때만 projection한다.

Riot 외부 match 상세는 내부 `games`에 넣지 않는다. 검증한 records page DTO와 recent page만 위 전용
PostgreSQL cache table에 저장한다. Private Riot integration service는 provider 호출/변환만 담당하고 같은
payload의 별도 장기 cache를 소유하지 않는다. 이 cache는 guild MMR source of truth가 아니다.

## 핵심 transaction

### 일반 game submit

```text
idempotency key 확보
-> series SELECT FOR UPDATE
-> expectedRevision 확인
-> game_input/picks/bans 조회
-> bestOf, next game number, winner, frozen roster 검증
-> ensure_and_lock_mmr_rows
-> games/game_participants/game_bans insert
-> mmr_ledger_entries append + user_lane_mmr update
-> series winner/status/revision update
-> checkpoint/audit/outbox/idempotency complete
-> commit
```

### 경매 낙찰

```text
idempotency key 확보
-> tournament SELECT FOR UPDATE
-> active lot/candidate lock
-> target team과 point summary lock
-> expectedRevision/status/roster 정원/잔여 point 검증
-> action/team_members/point_ledger append
-> candidate/lot/team summary update
-> checkpoint/audit/outbox/revision/idempotency complete
-> commit
```

### 경매 game submit

tenant-scoped read로 match의 tournament ID를 먼저 resolve한 뒤 tournament, match 순으로 lock하고
request의 `expectedRevision`은 frontend command와 같은 match revision 하나만 검증한다. Tournament는
client token을 추가로 요구하지 않고 lock한 현재 status/bracket/match 관계를 서버가 다시 검증한다.
이후 일반 game transaction의 MMR 절차와 다음 검증을 적용한다.

- bracket가 lock 상태다.
- match 양 팀이 확정되어 있다.
- 양 팀 lineup이 lock되었고 각 5 lane이 완성되어 있다.
- 입력 pick user가 locked lineup과 일치한다.
- match와 tournament projection/revision을 함께 갱신하고 둘의 새 revision으로 각각 invalidate를
  만든다.

### Admin MMR 수동 조정

```text
ACTIVE OPERATOR 권한 확인
browser idempotency key 확보/replay
-> guild_admin_state(scope=MEMBERS) lazy init + lock
-> expected MEMBERS revision 확인
-> target member와 ACTIVE season 확인
-> ensure_and_lock_mmr_rows
-> MANUAL_ADJUSTMENT ledger append
-> summary update
-> MEMBERS revision + 1
-> MEMBER_MMR_ADJUSTED audit + full member snapshot
-> idempotency response 완료
-> commit
```

Browser `Idempotency-Key`는 중복 없는 visible ASCII 1..128 bytes를 허용하고 공식 frontend는 UUID를
생성한다. Request hash에는 guild/member와 역직렬화한 request body 전체가 포함되며 24시간 동안 replay한다.
Note는 저장 전에 trim하고 trim 결과가 1..500자인지 검증한다. Replay는 저장된 full snapshot을 반환하며
revision을 다시 비교하거나 ledger를 추가하지 않는다. `user_lane_mmr`의 MMR/revision/updated_at만
갱신하고 games/wins는 바꾸지 않는다. Audit target type은 `GUILD_MEMBER_MMR`이고 before/after에는 lane
MMR과 lane revision을 기록한다.

### Admin member 상태 변경

```text
ACTIVE OPERATOR 권한 확인
-> guild_admin_state(scope=MEMBERS) lazy init + lock
-> expected MEMBERS revision 확인
-> target guild_members row lock
-> status와 inactivated_at 갱신
-> MEMBERS revision + 1
-> MEMBER_STATUS_CHANGED audit + full member snapshot
-> commit
```

INACTIVE에서는 `inactivated_at=now()`, ACTIVE 복구에서는 null로 갱신한다. `registered_at`, 기존
`user_lane_mmr`, game/MMR ledger는 보존한다. Status set은 revision으로 충분하므로 idempotency key를
요구하지 않는다. Audit target type은 `GUILD_MEMBER`다.

Admin members 화면은 WebSocket topic이 없으므로 이 mutation만을 위한 invalidate outbox를 만들지
않는다. Admin에서 active series/tournament 자체를 revert한 경우에는 변경된 domain aggregate의
topic으로 outbox event를 기록한다.

일반 domain command의 checkpoint INSERT는 guild 전역 operations row를 lock하지 않는다.
`APPLY_REVERT`와 명시적 expire만 `guild_admin_state(scope=OPERATIONS)`를 lock하고 revision과
metadata를 갱신한다. APPLY는 checkpoint row를 `FOR UPDATE`로 잠그고 `status=ACTIVE`와 target
dependency를 다시 검증하므로 새 checkpoint 추가와 무관하게 stale 복구가 성공하지 않는다.

외부 Discord/Riot/Valkey call은 위 transaction 밖에서 실행한다.

aggregate 생성, ledger append, 결과 확정·취소·되돌리기처럼 `Idempotency-Key`가 필수인 command만
idempotency unique row를 먼저 확보한다. entry 이동, pick/ban cell 편집, setup field 편집처럼
revision으로 충분한 command는 이 단계를 생략한다. 이후 해당 command가 admin MEMBERS/OPERATIONS
revision을 변경할 때만 그 scope row를 잠그고, command 종류별로 다음 순서를 사용한다.

경매 `DRAW_NEXT`, `MARK_UNSOLD`, `AWARD_BID`, `ASSIGN_CURRENT`, `ASSIGN_UNSOLD`,
`RESTORE_UNSOLD`, `REVERT_LAST`는 모두 action ledger를 append하므로 예외 없이 key가 필수다.
Tournament cancel도 terminal state 전이이므로 key가 필수다.

```text
일반 모집 편집
  recruitment -> entry draft/assignment

일반 series 생성
  recruitment -> entry draft/assignment -> series INSERT

경매 모집/setup 편집
  auction recruitment -> setup draft -> setup team을 team_index 순

경매 tournament 생성
  auction recruitment -> setup draft -> setup team을 team_index 순
  -> tournament/team/candidate INSERT

source 모집과 생성된 child를 함께 바꾸는 취소/재개/생성 되돌리기
  source recruitment -> series 또는 tournament -> 관련 draft/child

생성 이후 일반 series command
  series -> child -> user_lane_mmr를 (user_id, lane) 순

생성 이후 경매 command
  tournament -> affected match를 UUID 순 -> team/child를 UUID 또는 team_index 순
  -> user_lane_mmr를 (user_id, lane) 순
```

기존 series/tournament command가 뒤늦게 source recruitment를 잠그는 순서는 금지한다. 모든 필요한
row lock이 끝난 뒤 ledger/audit와, WS topic이 있는 aggregate에 한해 outbox를 append한다. direct
auction match endpoint도 tenant read로 tournament ID를 먼저 resolve한 뒤 tournament부터 잠근다.

기본 isolation은 `READ COMMITTED`이며 row lock, revision, unique constraint를 조합한다. 전체 DB에
`SERIALIZABLE`을 적용하지 않는다.

## Delete와 보존 정책

Hard cascade 허용:

- 만료 OAuth attempt와 session
- user privacy/default/champion preference
- guild member role cache
- 아직 원장으로 확정되지 않은 game input child
- auction setup draft child와 recruitment entry assignment

Hard cascade 금지:

- series, tournament, match, game
- game participant/ban과 MMR ledger
- auction lot/action/point ledger/team membership
- checkpoint와 audit

운영 domain은 `CANCELLED`, `VOIDED`, `REVERSED`, `released_at`, `deleted_at` 상태를 사용한다.
FK는 기본 `RESTRICT`다. 실제 개인정보 purge가 필요하면 offline purge job이 dependency order와
audit를 명시하고 실행한다.

Retention:

- OAuth attempts: 만료 후 즉시 cleanup
- revoked/expired sessions: 30일
- idempotency response: 기본 24시간
- published WebSocket outbox: 7일
- delivered notification outbox: 30일
- terminal failed notification outbox: 운영 확인 후 90일
- audit/MMR/auction ledger: 운영 보존, 자동 삭제 없음

## 필수 index

PK/UQ 외에 첫 migration부터 다음 index를 둔다. PostgreSQL은 FK child column index를 자동으로
만들지 않으므로 모든 composite FK의 child 쪽 leading index를 확인한다.

```text
guild_members
  (guild_id, status, last_active_at DESC)
  (guild_id, lower(display_name))
  (user_id, status, guild_id)

riot_accounts
  (user_id, created_at)

oauth_login_attempts / user_sessions / idempotency_records
  oauth_login_attempts(expires_at)
  user_sessions(expires_at)
  idempotency_records(expires_at)

recruitments / auction_recruitments
  (guild_id, status, updated_at DESC)
  (guild_id, season_id, created_at DESC)

series / auction_tournaments
  (guild_id, status, updated_at DESC) WHERE deleted_at IS NULL
  (guild_id, season_id, started_at DESC) WHERE deleted_at IS NULL

recruitment_participants / auction_recruitment_participants
  (guild_id, user_id, joined_at DESC)

series_participants
  (guild_id, user_id, series_id)

auction_team_members
  (guild_id, user_id, tournament_id)

auction_bid_candidates
  (guild_id, tournament_id, state, queue_position)

auction_actions
  (guild_id, tournament_id, sequence DESC)

auction_point_ledger
  (guild_id, tournament_id, team_id, created_at DESC)

auction_matches
  (guild_id, tournament_id, status, updated_at DESC) WHERE deleted_at IS NULL

games
  (guild_id, season_id, confirmed_at DESC) WHERE status='CONFIRMED'
  (guild_id, series_id, game_number) WHERE status='CONFIRMED'
  (guild_id, auction_match_id, game_number) WHERE status='CONFIRMED'

game_participants
  (guild_id, user_id, game_id)
  (guild_id, user_id, lane, game_id)
  (guild_id, champion_id, game_id) WHERE champion_id IS NOT NULL

mmr_ledger_entries
  UQ (guild_id, season_id, user_id, lane, lane_revision)
  (guild_id, season_id, user_id, lane, created_at DESC, id DESC) for time-range history
  (guild_id, game_id) WHERE game_id IS NOT NULL

user_lane_mmr
  (guild_id, season_id, lane, mmr DESC, user_id)

admin_audit_log
  (guild_id, created_at DESC, id DESC)
  (guild_id, action, created_at DESC)
  (guild_id, principal_id, created_at DESC)

outbox_events
  (available_at, id) WHERE published_at IS NULL
  (lease_until) WHERE published_at IS NULL AND lease_until IS NOT NULL

bot_notification_outbox
  (available_at, id) WHERE delivered_at IS NULL AND failed_at IS NULL
  (lease_until)
    WHERE delivered_at IS NULL AND failed_at IS NULL AND lease_until IS NOT NULL
```

Riot ID는 normalized exact/prefix search부터 적용한다. Guild display name의 부분 검색이 실제로
필요하고 dataset에서 효용이 확인되면 `pg_trgm` GIN index를 `0009_read_indexes`에서 추가한다.
모든 index는 대표 query의 `EXPLAIN (ANALYZE, BUFFERS)`로 검증한다.

초기 partitioning은 하지 않는다. MMR/audit/outbox가 수천만 row에 도달하고 cleanup과 index
tuning으로 해결되지 않을 때 월 단위 partition을 별도 설계한다.

## Migration 순서

FK dependency가 성립하는 순서로 migration을 고정한다.

```text
0001_bootstrap.sql
  database prerequisites, optional pg_trgm, helper comments

0002_identity_auth_riot.sql
  users, oauth_login_attempts, user_sessions
  riot_accounts, user_privacy_settings, user_champion_preferences

0003_guild_members_seasons_config.sql
  guilds, guild_members, guild_member_discord_roles
  guild_config, guild_admin_state, seasons, guild_counters
  user_guild_preferences

0004_recruitments_entry.sql
  recruitments, participants, preferred lanes
  recruitment_entry_drafts, assignments

0005_series_core.sql
  series, series_participants

0006_auction_core_bracket_matches.sql
  auction recruitments/setup/tournament/team/member
  candidate/lot/action/point ledger
  bracket slots, matches, lineups

0007_game_inputs_games_mmr.sql
  game_inputs, input picks/bans
  games, participants, optional stats, bans
  user_lane_mmr, mmr_ledger_entries

0008_audit_checkpoint_idempotency_outbox.sql
  revert_checkpoints, admin_audit_log
  idempotency_records, outbox_events, bot_notification_outbox, bot_instances

0009_read_indexes.sql
  non-constraint read indexes and measured search indexes

0010_riot_records_snapshots.sql
  canonical records snapshots, Riot ID aliases, recent pages, refresh gates

0011_guild_settings_discord_catalog.sql
  guild_config entry channel, Discord role/channel catalogs and sync state

0012_user_identity_kinds.sql
  provider-neutral user ID, DISCORD/DUMMY identity kind, Discord identity mapping

0013_dummy_users.sql
  persistent dummy profiles/groups/audit, synthetic Riot account origin

0014_riot_account_owner_identity.sql
  Riot account origin and owner identity kind composite FK

0015_series_hard_fearless.sql
  recruitment/series hard fearless rule snapshot

0016_guild_result_channel.sql
  guild_config explicit series-result channel without entry-channel fallback
```

`games.auction_match_id`가 참조할 table보다 먼저 games를 생성하지 않는다. migration file은
forward-only이며 API process startup에서 자동 실행하지 않는다. 배포 전 one-shot migration job이
실행한다.

SQLx 운영 기준:

- 빈 PostgreSQL에 모든 migration을 적용하는 integration test
- 최신 schema와 한 version 전 schema에서 upgrade test
- `query!` 계열 compile-time macro를 도입한 시점부터 `.sqlx` offline metadata를 commit하고
  `cargo sqlx prepare --check`를 적용한다. 동적 `query/query_as/query_scalar`만 사용하는 현재 단계는
  실제 PostgreSQL integration test로 SQL과 schema 계약을 검증한다.
- migration role과 runtime role 분리
- runtime role의 DDL/TRUNCATE 금지
- ledger/audit UPDATE/DELETE 금지
- 고정 `search_path`
- `lock_timeout`, `statement_timeout`, `idle_in_transaction_session_timeout` 설정
- migration 전 backup, 실제 restore rehearsal

## mookbot_v3 migration

Source:

```text
../mookbot_v3/packages/core/src/db/schema.sql
../mookbot_v3/packages/core/src/mmr/*
../mookbot_v3/packages/core/src/kv/routed.ts
```

v3는 SQLite/D1 + 선택적 Valkey routed draft, 단일 guild, numeric ID, unix second timestamp
구조다. source DB/keyspace를 직접 변경하지 않고 read-only snapshot을 export한다.

### Import support

Import 때만 다음 보조 구조를 사용한다.

```text
migrations/legacy/0001_import_support.sql
```

이 migration은 core `0001..0011`과 분리한다. Phase 11 rehearsal/cutover import job만 대상 DB에
적용하며 API startup, 일반 deploy migration에는 포함하지 않는다. Mapping과 quarantine은 cutover
감사를 위해 보존하되 runtime role에는 권한을 주지 않는다.

```text
legacy_import_runs
  id                uuid        PK
  target_guild_id   text        NOT NULL FK -> guilds.guild_id RESTRICT
  source_checksum   bytea       NOT NULL CHECK octet_length=32
  manifest          jsonb       NOT NULL
  state             text        NOT NULL CHECK RUNNING|VALIDATING|COMPLETED|FAILED
  report            jsonb       NULL
  last_error        text        NULL
  started_at        timestamptz NOT NULL
  updated_at        timestamptz NOT NULL
  finished_at       timestamptz NULL

  UQ (target_guild_id, source_checksum)
  partial UQ (target_guild_id) WHERE state IN ('RUNNING','VALIDATING')
  CHECK ((state IN ('RUNNING','VALIDATING') AND finished_at IS NULL)
      OR (state IN ('COMPLETED','FAILED') AND finished_at IS NOT NULL))

legacy_entity_maps
  import_run_id uuid        NOT NULL FK -> legacy_import_runs.id RESTRICT
  entity_type   text        NOT NULL CHECK length > 0
  legacy_id     text        NOT NULL CHECK length > 0
  guild_id      text        NOT NULL FK -> guilds.guild_id RESTRICT
  new_id        uuid        NOT NULL
  created_at    timestamptz NOT NULL

  PK (import_run_id, entity_type, legacy_id)
  UQ (import_run_id, guild_id, new_id)

legacy_quarantine
  id               uuid        PK
  import_run_id    uuid        NOT NULL FK -> legacy_import_runs.id RESTRICT
  import_unit_type text        NOT NULL CHECK length > 0
  import_unit_id   text        NOT NULL CHECK length > 0
  entity_type      text        NOT NULL CHECK length > 0
  legacy_id        text        NOT NULL CHECK length > 0
  reason_code      text        NOT NULL CHECK length > 0
  severity         text        NOT NULL CHECK ERROR|WARNING
  resolution       text        NOT NULL CHECK OPEN|REMEDIATED|ACCEPTED_OMISSION|ACCEPTED_REENTRY
  source_payload   jsonb       NOT NULL
  details          jsonb       NULL
  created_at       timestamptz NOT NULL
  resolved_by      text        NULL
  resolved_at      timestamptz NULL

  UQ (import_run_id, entity_type, legacy_id, reason_code)
  CHECK ((resolution='OPEN' AND resolved_by IS NULL AND resolved_at IS NULL)
      OR (resolution<>'OPEN' AND resolved_by IS NOT NULL AND resolved_at IS NOT NULL))
```

원본 raw export는 별도 `legacy_v3` staging schema에 넣고 application runtime role에는 접근 권한을
주지 않는다. `legacy_id`는 source table PK를 canonical decimal/text로, composite key면
`column=value`을 column 이름 순으로 연결해 만든다. mapping row는 insert-only다. 같은 guild/source
checksum import는 하나의 run만 만들고 FAILED run은 같은 ID/mapping으로 재개한다. state transition은
`RUNNING -> VALIDATING -> COMPLETED`, 실패 시 `FAILED`, 명시적 resume만 `FAILED -> RUNNING`이다.
다른 checksum도 같은 guild에서 동시에 RUNNING/VALIDATING이 될 수 없다.

### Import manifest와 prerequisite

v3 source에는 Discord guild root와 운영자 role 설정이 없으므로 rehearsal/cutover마다 다음
manifest를 필수 입력으로 받는다. 값은 source content가 아니라 운영 배포 설정이다.

```text
target_guild_id          Discord guild snowflake string
target_guild_name        guild/config display name
operator_role_id         Discord role snowflake string
legacy_platform          explicit Riot platform, current deployment default KR
legacy_routing           explicit Riot routing, current deployment default ASIA
active_legacy_season_id  required only when v3 has multiple open seasons
bootstrap_season_name    required only when v3 has no open season
draft_store              VALKEY or D1, copied from frozen v3 runtime config
write_freeze_at          timestamp after v3 mutation endpoints stop
d1_snapshot_checksum     SHA-256 of the immutable D1/SQLite export
d1_schema_fingerprint    SHA-256 of canonical table/column/index/FK metadata
d1_table_counts          exact row count map for every required source table
d1_table_checksums       per-table canonical JSONL SHA-256 map
valkey_snapshot_checksum SHA-256 of entry/pickban JSONL when draft_store=VALKEY
datadragon_version       pinned champion catalog version used for normalization
champion_catalog_checksum SHA-256 of the pinned catalog artifact
recruitment_channel_id   optional
log_channel_id           optional
source_checksum          SHA-256 of canonical manifest excluding this field + artifact checksums
```

Core migration 0001..0011이 적용된 빈 target DB를 기본으로 한다. target guild가 이미 있으면
`guild_config` 설정이 manifest와 정확히 일치하고 domain row가 하나도 없는 경우만 허용하며, 기존
설정이나 데이터를 import job이 덮어쓰지 않는다. application이 run UUID와 started_at을 먼저
고정하고, 한 transaction에서 guild root를 먼저 만든 뒤 `legacy_import_runs`를 insert한다.

```text
guilds(discord_name=manifest name, status=ACTIVE, installed_by=NULL,
       installed/activated/updated_at=import run started_at)
guild_config(display/operator/channels=manifest, is_externally_public=false,
             revision=0, updated_by=NULL, updated_at=import run started_at)
guild_admin_state(scope=MEMBERS, revision=0, updated_by=NULL, updated_at=import run started_at)
guild_admin_state(scope=OPERATIONS, revision=0, updated_by=NULL, updated_at=import run started_at)
```

`installed_by`는 source에서 알 수 없으므로 NULL이다. 이후 user, member, season, domain 순으로
적재한다. 동일 checksum/guild 조합의 성공 run이 이미 있으면 새 import를 시작하지 않고 완료
결과를 반환한다.

cutover는 v3 API/Bot/worker/cleanup job의 모든 source mutation을 global maintenance mode로 막고
export job을 SELECT/PRAGMA allowlist로 제한한 뒤 시작한다. 등록/Riot 계정/챔피언 선호/시즌/MMR/admin audit/
soft-delete를 포함해 아래 24개 table write는 전부 금지하며 GET/health만 허용한다. Valkey도
`entry:*`, `pickban:*` write를 막고 export 대상이 아닌 `cache:*` write만 허용할 수 있다. 그 뒤
D1과 Valkey를 export한다. `draft_store=VALKEY`면 `SCAN` 기반으로 `entry:*`, `pickban:*` key/value를 JSONL로
내보내며 `cache:*`는 제외한다. Valkey value에는 신뢰할 timestamp가 없으므로 D1의 같은 prefix
row와 시각 비교하거나 fallback하지 않고 frozen runtime의 routed store를 권위 source로 사용한다.
`draft_store=D1`일 때만 D1 `guild_kv`의 두 prefix를 읽는다. export가 끝날 때까지 write freeze를
유지하며 freeze 이후 source write가 발견되면 checksum을 폐기하고 처음부터 다시 export한다.

기존 `packages/core/src/db/dump.ts::dumpDatabase()`는 경매/pick/ban/champion preference table을
누락하므로 migration source로 사용하지 않는다. raw D1 SQL export 또는 `sqlite_master`에서 검증한
table별 canonical JSONL export만 허용한다. 다음 24개 table과 `sqlite_sequence`가 모두 manifest
count/checksum에 있어야 한다.

```text
users, riot_accounts, seasons, series, series_participants, games, game_stats,
user_lane_mmr, mmr_changes, recruitments, recruitment_participants,
recruitment_participant_roles, admin_audit_log, guild_kv, game_picks, game_bans,
user_champion_preferences, auction_recruitments, auction_recruitment_participants,
auction_tournaments, auction_teams, auction_team_members, auction_bids, auction_matches,
sqlite_sequence
```

logical schema fingerprint는 정렬한 `PRAGMA table_info/index_list/foreign_key_list` 결과로 계산하고
지원 fingerprint allowlist와 일치해야 한다. `PRAGMA integrity_check`와 `foreign_key_check`, export
후 table별 row count 재조회도 통과해야 staging import를 시작한다.

### Mapping

| v3                               | PostgreSQL                                          |
| -------------------------------- | --------------------------------------------------- |
| Discord user ID                  | `user_discord_identities.discord_id`에 그대로 보존  |
| numeric season ID                | UUIDv7 + `legacy_entity_maps`                       |
| series/recruitment 동일 ID       | 서로 다른 UUID, `series.source_recruitment_id` 연결 |
| tournament/recruitment 동일 ID   | 서로 다른 UUID, source FK 연결                      |
| positive numeric domain ID       | entity별 `display_number`로 보존                    |
| unix integer timestamp           | `to_timestamp(...)` UTC                             |
| `REAL` MMR                       | `double precision`, rounding 없음                   |
| role                             | canonical lane, `ADC` 발견 시 `BOTTOM` 명시 mapping |
| champion numeric/name            | Data Dragon slug로 normalize                        |
| game `team1_side`/`winning_team` | `side_preset`/`winning_side`로 변환                 |
| `game_stats`                     | participant + optional participant stats            |
| `games.riot_match_id`            | `games.riot_match_id` link만 보존, payload는 미이전 |
| queue JSON                       | normalized candidate rows                           |
| routed `entry/pickban` draft     | frozen authoritative KV export를 normalized row로   |
| D1 `auction-lineup:*`            | tournament assignment를 known match lineup으로      |

Game 변환:

```text
team1_side=BLUE  -> side_preset=DEFAULT
team1_side=RED   -> side_preset=SWAPPED

winning_team=TEAM_1 + team1_side=BLUE -> winning_side=BLUE
winning_team=TEAM_1 + team1_side=RED  -> winning_side=RED
winning_team=TEAM_2 + team1_side=BLUE -> winning_side=RED
winning_team=TEAM_2 + team1_side=RED  -> winning_side=BLUE
```

### Mandatory target field synthesis

v3에 없지만 신규 NOT NULL/FK 제약에 필요한 값은 import job이 다음 규칙으로만 합성한다.
합성 actor ID는 `migration:<legacy_import_runs.id>`로 고정하고 actor type은 `MIGRATION`으로
기록한다. UUID는 먼저 `legacy_entity_maps`에 기록한 뒤 재시도에서 그 mapping을 재사용한다.

| Target field/row                                          | Deterministic import rule                                                                                           |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| four domain `title` fields                                | 일반은 `일반 내전 #N`, 경매는 `10인 경매 내전 #N` 또는 `20인 경매 내전 #N`                                          |
| four domain `display_number` fields                       | 해당 source entity의 양수 numeric ID를 그대로 보존                                                                  |
| domain workspace/draft `revision`                         | imported snapshot은 `0`; import 이후 첫 mutation부터 증가                                                           |
| `user_lane_mmr.revision`                                  | imported ledger의 마지막 lane_revision, entry가 없으면 0                                                            |
| `series.best_of`, `auction_matches.best_of`               | 일반 series는 3; v3 match `format=BO1/BO3`은 각각 1/3                                                               |
| `auction_teams.team_name`, `created_at`                   | trim한 source 이름 또는 `팀 <team_index>`; 생성 시각은 tournament `started_at`                                      |
| captain `auction_team_members` row                        | `roster_slot=1`, `acquired_via=CAPTAIN`, `joined_at=tournament.started_at`                                          |
| non-captain member slot/time                              | final bid `(created_at,id)`, 이후 MANUAL `user_id` 순으로 slot 2..5; BID 시각은 bid 시각, MANUAL은 parent 시작 시각 |
| final v3 bid의 candidate/lot/action                       | ASSIGNED candidate, closed AWARDED lot, `DRAW_NEXT`와 `AWARD_BID` action을 합성해 member/point와 연결               |
| v3 MANUAL member의 candidate/lot/action                   | ASSIGNED candidate, closed ASSIGNED lot, `DRAW_NEXT`와 `ASSIGN_CURRENT`를 합성해 member/action을 연결               |
| current bid target의 candidate/lot/action                 | ACTIVE candidate, open ACTIVE lot, `DRAW_NEXT` action을 합성                                                        |
| remaining candidate queue                                 | source order, 또는 lazy NULL/empty면 `(joined_at,user_id)` 합성 순서로 QUEUED                                       |
| candidate `draw_count`, `updated_at`                      | 합성 lot 수, 마지막 source event 시각; event가 없으면 tournament `started_at`                                       |
| lot/action/point actor                                    | `actor_type=MIGRATION`, `actor_id=migration:<import_run_id>`                                                        |
| auction point revisions                                   | INITIAL/team summary는 0, BID_DEBIT/REVERSAL event 순으로 1씩 증가                                                  |
| `games.confirmed_by`, `confirmed_by_type`, `confirmed_at` | migration actor ID, `MIGRATION`, source `played_at`                                                                 |
| synthetic `game_inputs` metadata                          | `updated_by=NULL`, `updated_at=played_at`, `status=SUBMITTED`                                                       |
| source에 없는 `updated_by`                                | nullable column이면 NULL; 사용자 ID를 임의 추측하지 않음                                                            |
| source에 없는 timestamp                                   | source event, parent start, import run `started_at` 순의 fallback을 쓰고 reconciliation report에 기록               |

모든 non-captain candidate의 `queue_position`은 source queue 순번, current target, final bid의
`(created_at,id)`, MANUAL `user_id` class 순으로 중복 없이 0부터 부여한다. captain, team member,
current target을 source queue에서 다시 만나면 source inconsistency로 quarantine한다. 합성 lot의
`draw_sequence`는 tournament 안의 final bid, MANUAL member, current target을 source event time,
`legacy_bid_id NULLS LAST`, `target_user_id` 순으로 정렬해 1부터 부여한다. action의 `sequence`는
모든 합성 action을 `(created_at, kind_order, legacy_bid_id NULLS LAST, target_user_id)`로 정렬하며
kind order는 `DRAW_NEXT=1`, `AWARD_BID=2`, `ASSIGN_CURRENT=3`이다. v3 final bid가 한 target에
여러 개이거나 BID member/team/final bid가 서로 맞지 않으면 하나를 임의 선택하지 않고
quarantine한다. 팀이 captain 포함 5명을 넘거나 roster 순서를 유일하게 만들 수 없는 경우도
quarantine한다. 합성 MANUAL action은 과거 상태를 신규 command로 재실행한다는 뜻이 아니라, 새
member FK와 감사 원장을 만족시키는 `MIGRATION` provenance다.

Additional rules:

- source의 양수 numeric ID를 각 entity의 `display_number`로 그대로 보존한다. source ID가
  비양수이거나 entity 내부에서 중복되면 임의 재번호를 부여하지 않고 quarantine한다.
- v3 season은 `ended_at IS NOT NULL`이면 CLOSED다. open season이 하나면 그 row만 ACTIVE로
  옮긴다. 여러 개면 manifest의 `active_legacy_season_id`가 가리키는 open row만 ACTIVE로 두고
  나머지는 CLOSED, `ended_at=import run started_at`으로 합성해 `INFERRED_END_AT`을 보고한다. open
  season이 없으면 모든 source season을 CLOSED로 옮기고 manifest의 `bootstrap_season_name`으로
  import 시각에 새 ACTIVE season을 만든다. 다중 open인데 selection이 없거나 closed season을
  선택하면 import를 시작하지 않는다.
- v3 Riot ID도 runtime `normalize_riot_id_v1`을 적용한다. 서로 다른 PUUID가 같은 normalized
  routing/name/tag로 충돌하면 관련 user의 Riot account import unit 전체를 quarantine한다. user별
  source main이 정확히 하나면 보존하고, 0개면 `(created_at, puuid)`가 가장 앞선 account를 main으로
  선택해 `INFERRED_MAIN`을 보고한다. main이 여러 개면 임의 선택하지 않고 account unit 전체를
  quarantine한다. account가 없는 user는 main row가 없어도 된다.
- USER_ACCOUNT transaction을 시작하기 전에 staging의 모든 normalized account와 target DB에 이미
  존재하는 global account를 한 번에 group해 PUUID/Riot ID collision 및 per-user main cardinality를
  확정한다. 충돌 graph에 포함된 모든 user unit을 먼저 표시한 뒤 non-conflict account만 insert해,
  순차 import 중 첫 user만 commit되는 비대칭 quarantine을 금지한다.
- live series/tournament마다 같은 numeric ID의 source recruitment가 정확히 하나 존재하고
  recruitment의 converted FK가 NULL 또는 해당 child ID인지 preflight한다. parent가 없거나 converted
  link가 다른 child를 가리키면 synthetic recruitment를 만들지 않고 parent aggregate import unit
  전체를 quarantine한다. v3 AUTOINCREMENT fallback으로 생성된 orphan도 같은 정책이다.
- 일반 game별 `mmr_changes.season_id`는 10명 모두 하나여야 한다. series의 모든 game이 같은 MMR
  season이면 그 season을 target recruitment/series/game/ledger의 canonical season으로 사용하고,
  source recruitment/series season과 다르면 `LEGACY_RESULT_SEASON_NORMALIZED`로 원본을 report한다.
  game이 없으면 source series season을 쓰고 recruitment만 그 값으로 normalize한다. 한 series의
  games가 둘 이상 MMR season에 걸치면 현재 composite FK/시즌별 MMR 의미로 안전하게 표현할 수
  없으므로 SERIES unit ERROR다.
- v3 auction tournament는 생성 시 current season을 다시 읽으므로 recruitment와 season이 달라도
  tournament season을 target recruitment/tournament/match/game의 canonical season으로 사용하고
  `LEGACY_AUCTION_SEASON_NORMALIZED`를 report한다. 과거 경매 MMR은 적용하지 않으므로 별도 MMR
  season 변환은 없다.
- 일반 recruitment `target_count`가 10이 아니거나 live series roster가 양 팀 5 lane을 만족하지
  않으면 현재 5v5 제품 계약으로 강제 변환하지 않고 RECRUITMENT/SERIES unit ERROR로 둔다. 완료된
  또는 취소된 historical non-5v5만 명시적 `ACCEPTED_OMISSION`이 가능하다.
- v3 COMPLETED series는 game score가 best-of 승수를 채우면 SCORE_REACHED, winner가 있지만 승수를
  못 채웠으면 EARLY_WIN, winner가 NULL이고 score가 동률이면 EARLY_DRAW로 mapping한다. NULL winner
  비동률은 ERROR다. CANCELLED는 terminal_reason=CANCELLED다. terminal `ended_at`이 source에 없으면
  matching audit 시각, 없으면 import run started_at을 합성해 report한다.
- v3에서 `deleted_at IS NOT NULL`인 series/tournament와 그 child match/game/stat/pick/ban은
  runtime schema로 가져오지 않는다. 시즌 결과 reset과 운영 삭제가 이미 사용자 조회에서 숨긴
  계층이며, 일부는 `mmr_changes`가 hard-delete되어 APPLIED ledger를 재구성할 수 없다. raw row와
  parent-child count는 staging 및 `LEGACY_SOFT_DELETED_OMITTED` report에 보존한다. 이 parent를
  잃은 recruitment가 CONVERTED이면 CLOSED, `converted_at=NULL`로 mapping한다.
- live 일반 game은 참가자별 v3 `mmr_changes`가 완전할 때만 `mmr_status=APPLIED`로 import한다.
  누락/중복 원장이 있는 live game은 임의 재계산하지 않고 quarantine한다. soft-deleted 계층을
  제외한 뒤 live GAME_RESULT 사이의 before/after gap과 source current summary의 tail 차이는
  `MIGRATION_SEED`로 명시적으로 연결한다.
- v3 일반 모집/경매 모집 participant는 모두 `JOINED`로 옮긴다. withdraw 이력이 source에 없으므로
  임의로 복원하지 않는다. `recruitment_participant_roles`는 source에 우선순위가 없으므로
  `TOP, JUNGLE, MID, BOTTOM, SUPPORT` canonical order로 정렬해 priority 1..N을 합성하고
  reconciliation report에 `INFERRED_ORDER`로 기록한다.
- authoritative draft export의 `entry:{recruitmentId}`는 JSON object와 participant/slot uniqueness를
  검증해 `recruitment_entry_drafts(revision=0, updated_by=NULL, updated_at=write_freeze_at)` 및
  assignment row로 변환한다. `TEAM_1_TOP` 같은 v3 slot ID는 team/lane으로 parse한다. invalid
  active draft는 recruitment 원장을 버리지 않고 ERROR로 두며 운영자가 고치거나
  `ACCEPTED_REENTRY`로 빈 draft 재입력을 승인해야 한다.
- 모든 imported recruitment에는 KV 존재 여부와 무관하게 draft root를 만든다. authoritative
  `entry:*` key가 없는 OPEN/CLOSED recruitment는 정상 미편집 상태이므로 revision 0, 빈 assignments,
  `updated_at=recruitment.updated_at`, `updated_by=NULL`로 합성한다. missing key는 invalid JSON과 같은
  quarantine reason으로 취급하지 않는다.
- live series가 있는 recruitment의 entry draft는 authoritative KV가 아니라 frozen
  `series_participants`에서 반드시 재구성한다. series 생성 뒤 v3가 entry key를 삭제하기 때문이다.
  이 draft는 series `started_at`, revision 0을 쓰며, imported zero-game series를 생성 revert했을 때
  같은 10명/team/lane assignment로 돌아갈 수 있어야 한다.
- authoritative `pickban:{seriesId}`의 v3 `games[]`를 set별 `game_inputs(DRAFT)`로 변환한다.
  `team1Side=BLUE/RED`는 DEFAULT/SWAPPED이며 picks array index는 canonical lane 순으로 frozen
  participant user를 찾고, bans TEAM_1/TEAM_2는 해당 side로 바꾼다. 이미 confirmed된 set은
  synthetic SUBMITTED input과 비교만 하고 중복 DRAFT를 만들지 않는다. side가 NULL인 set은 target
  row를 만들 수 없으므로 raw draft를 보존하고 `ACCEPTED_REENTRY` 승인을 요구한다. invalid champion,
  roster, duplicate slot은 SERIES unit ERROR다.
- v3 `user_champion_preferences`는 user/lane 안에서 `(position, normalized champion_id)` stable
  sort 후 position을 1..N으로 다시 부여한다. normalize 뒤 champion이 충돌하거나 10개를 넘으면
  자르거나 하나를 선택하지 않고 quarantine한다.
- v3 game 1..3은 그대로 옮기며 신규 schema는 1..5를 허용한다.
- 모든 v3 game마다 같은 parent, set number, side, winner를 가진 synthetic
  `game_inputs(status=SUBMITTED)`를 먼저 만들고 v3 `game_picks`/`game_bans`를 input child로
  정규화한다. 확정 game은 이 synthetic input을 `source_input_id`로 참조한다.
- v3 pick의 `(team, role)`은 일반 game이면 frozen `series_participants`, 경매 game이면 같은
  game의 `game_stats`와 일관된 inferred lineup을 통해 정확히 한 user로 resolve한다. explicit
  `game_picks`가 없으면 `game_stats.champion_id`만 confirmed participant에 보존한다. 두 source가
  서로 다른 champion을 가리키거나 user가 없거나 둘 이상이면 quarantine한다.
- v3 ban의 TEAM_1/TEAM_2는 `team1_side`로 BLUE/RED에 변환하고 `position`은 slot 1..5로
  보존한다. 변환한 pick/ban은 synthetic input child와 confirmed participant/ban 원장 양쪽에
  동일하게 반영한다.
- imported confirmed game은 일반 frozen roster 또는 경매 inferred/locked lineup의 정확한 10명과
  team/lane이 일치해야 한다. auction game_stats가 없거나 참가자가 불완전해 lineup을 복원할 수
  없으면 TOURNAMENT unit ERROR이며 빈 participant를 합성하지 않는다.
- `series_participants.mmr_at_lock`은 해당 user/lane의 첫 game `mmr_before`를 우선 사용한다.
  game이 없으면 import 시 current lane MMR을 `INFERRED`로 기록하고, 값 자체가 없으면 NULL과
  `UNAVAILABLE`로 남긴다.
- v3에 없던 경매 참가/후보/획득 lane/MMR snapshot은 game role, Riot main position, 현재 lane
  MMR 순으로 복원한다. 정확한 시점값이 아니면 `INFERRED`, 근거가 없으면 NULL과
  `UNAVAILABLE`을 기록한다. active tournament의 unresolved snapshot은 resume 전에 수동 해결한다.
- v3 users를 target guild의 `guild_members`로 seed한다. `users.deleted_at IS NULL`이면 ACTIVE,
  non-null이면 INACTIVE로 두고 registration timestamp를 보존한다. INACTIVE user를 진행 중
  aggregate가 참조하면 그 unit은 operator resolution 전 ERROR다. resolved main Riot account의
  main_position이 canonical lane이면 member main_lane으로 쓰고 FLEX/NULL이면 NULL이다.
- v3에 없는 privacy는 현재 `/me` 기본값인 `public_profile_enabled=true`, `show_riot_ids=true`,
  `allow_external_record_search=false`로 seed한다. 마지막 값은 길드 밖 custom match 전적 접근을
  허용하지 않는다는 뜻이며 public Riot records 검색과 무관하다. ACTIVE member의
  `user_guild_preferences`는 단일 target guild를 default로 두고 INACTIVE member는 default를 만들지
  않는다.
- v3에는 Discord role membership snapshot이 없으므로 `guild_member_discord_roles`를 합성하지
  않는다. cutover 전 Bot이 Discord에서 full member/role sync를 수행하고 manifest의 operator role을
  가진 ACTIVE member가 최소 한 명인지 검증한다.
- v3 일반 game MMR ledger는 `GAME_RESULT`, actor는 `MIGRATION`으로 옮긴다.
- user/season/lane별 GAME_RESULT를 source `(created_at,id)` 순으로 재생한다. 첫 `mmr_before`가
  1500이 아니거나 앞 entry의 `mmr_after`와 다음 `mmr_before`가 다르면 그 사이에
  `MIGRATION_SEED`를 넣고 reset/deleted game/manual legacy adjustment gap으로 report한다. 마지막
  tail과 imported `user_lane_mmr`의 MMR/game/win counter 차이도 final seed로 맞춘다. source sort로
  확정한 결과와 seed에 lane_revision 1..N을 순서대로 부여하며 UUID/timestamp를 tail 판정에 쓰지
  않는다.
- v3 경매는 MMR 0 정책이었으므로 game은 `LEGACY_NOT_APPLIED`로 옮기고 과거 MMR을 계산하지
  않는다. 신규 경매 game부터 `APPLIED`다.
- v3 경매 captain도 `auction_team_members(acquired_via=CAPTAIN)` row를 만든다.
- 모든 imported auction recruitment에는 `auction_setup_drafts(revision=0)`와 format별 team_index 1..2
  또는 1..4 row를 만든다. source setup/tournament team이 없는 정상 CAPTAIN_PICK은 `팀 N`,
  `captain_user_id=NULL`, 현재 frontend 기본값 `initial_points=1000`인 blank slot을 쓰고, 존재하는
  source team을 index별로 overlay한다. draft updated_at은 recruitment updated_at, updated_by는 NULL이다.
  source team index 중복/범위 초과나 captain participant 불일치는 AUCTION_SETUP/TOURNAMENT unit
  ERROR다.
- source tournament가 있는 recruitment의 `auction_setup_drafts/teams`는 tournament team의
  team_index/name/captain/initial_points에서 revision 0으로 재구성한다. tournament creation revert와
  CAPTAIN_PICK/POINT_ALLOC의 setup 복귀가 같은 snapshot을 사용해야 한다.
- v3는 initial point 0을 허용했지만 현재 frontend setup은 positive point만 완료 상태로 인정하므로
  source `initial_points<=0`을 1로 보정하지 않는다. active AUCTION_SETUP/TOURNAMENT는 operator가
  positive point로 명시적 remediation해야 하고, completed/cancelled historical tournament만
  `ACCEPTED_OMISSION`할 수 있다.
- 위 synthesis 규칙대로 BID/MANUAL member의 `acquisition_action_id`를 연결한다. 각 team에
  INITIAL point row를 만든 뒤 final bid를 BID_DEBIT ledger로 재생한다. 계산한 잔액과 v3
  `current_points`가 다르면 임의 seed를 만들지 않고 quarantine한다.
- v3 `auction_bids.is_final=0` row는 신규 frontend에 없는 bid intent/intermediate offer이므로 action
  원장으로 승격하지 않고 staging count/report에만 보존한다.
- v3 `CAPTAIN_PICK`/`POINT_ALLOC` tournament는 아직 tournament로 확정하지 않고 source auction
  recruitment=CLOSED와 `auction_setup_drafts`로 되돌려 import한다. `BIDDING`은 BIDDING,
  `PLACEMENT`/`BRACKET_SETUP`은 BRACKET_SETUP, `IN_GAME`/`COMPLETED`/`CANCELLED`는 같은 의미의
  신규 status로 mapping한다. CANCELLED terminal time이 없으면 matching audit, 없으면 import run
  started_at을 `INFERRED_END_AT`으로 사용한다.
- v3 auction match COMPLETED/CANCELLED는 같은 신규 status로 옮긴다. IN_PROGRESS match는 confirmed
  game이 있으면 IN_PROGRESS, game이 없고 유효 lineup이 있으면 READY, 없으면 LINEUP_PENDING이다.
  completed winner/end time과 game 다수결이 다르면 TOURNAMENT unit ERROR다. CANCELLED `ended_at`이
  없으면 matching audit 또는 parent tournament terminal time, 마지막으로 import run started_at을
  합성해 report한다.
- TEN_PERSON bracket slot A_0/A_1은 v3 SINGLE의 team1/team2에서 만든다. TWENTY_PERSON은 존재하는
  SEMI bracket_index 1/2만 A_0/A_1 또는 B_0/B_1에 복원하며 0-match BRACKET_SETUP과 1-semi
  IN_GAME도 정상 중간 상태로 허용한다. 0-match는 empty unlocked slots, 1-semi는 known pair만 채운
  unlocked bracket으로 import하고 나머지 두 team은 operator가 현재 setup UI에서 배치한다.
- 두 semifinal이 있으면 네 slot을 locked로 만들고 source FINAL이 아직 없어도 current frontend
  default인 best_of=3의 target FINAL placeholder를 합성한다. 완료된 SEMIFINAL_A/B winner는 즉시
  final left/right team으로 seed하고 미완료 side만 NULL로 두며 `SYNTHETIC_PENDING_FINAL`을
  report한다. imported partial bracket의 `LOCK_BRACKET`은 기존 match key/team이 slot과 일치하는지
  검증한 뒤 missing match만 생성해야 하며 기존 game을 덮어쓰지 않는다. team 중복, 잘못된
  bracket_index, existing match/slot 불일치만 TOURNAMENT unit ERROR다.
- v3 queue JSON이 non-empty면 순서를 보존하며 malformed/unknown/assigned/current/captain 중복은
  TOURNAMENT unit ERROR다. NULL/empty인데 아직 배치되지 않은 eligible participant가 있으면 v3의
  정상 lazy queue 상태로 보고 `(joined_at,user_id)` 순으로 QUEUED를 합성해
  `INFERRED_QUEUE_ORDER`를 남긴다. non-empty queue 밖에 eligible participant가 빠져 있으면 ERROR다.
- D1 `auction-lineup:{tournamentId}`는 team ID를 mapping한 뒤 각 team의 5개 canonical lane, active
  member 소속, tournament 내 user uniqueness를 검증한다. match에 games가 있으면 `game_stats`의
  team/lane/user가 모든 set에서 같은지 먼저 검증해 match별 historical lineup을 도출한다. set 간
  lane/user가 바뀌면 현재 fixed match lineup 계약으로 표현할 수 없으므로 TOURNAMENT unit ERROR다.
- COMPLETED 또는 games가 있는 CANCELLED match는 game-derived lineup을 canonical로 쓰고
  tournament-wide KV와 다르면 `STALE_LEGACY_LINEUP` warning을 남긴다. active match에서 이미 기록된 game lineup과 KV가 다르면
  future resume 의미가 불명확하므로 ERROR, game이 없으면 유효 KV를 사용한다. KV가 없지만
  game-derived lineup이 있으면 이를 `INFERRED_LINEUP`으로 쓴다. BIDDING처럼 target match가 아직
  없는 key는 raw 값을 보존하고 신규 per-match UI 재입력에 `ACCEPTED_REENTRY`가 필요하다.
- lock 시각은 D1 `guild_kv.updated_at`, 없으면 첫 game `played_at`, 그마저 없으면 tournament
  `started_at` 순으로 사용한다. 최신 `lineup-set` audit operator가 imported ACTIVE guild member일
  때만 `lineups_locked_by`로 쓰고 아니면 tournament creator로 fallback해 `INFERRED_LOCK`을
  report한다. malformed lineup 또는 source stage상 locked여야 하는데 KV/game 양쪽에서 복원할 수
  없는 lineup은 TOURNAMENT unit ERROR다.
- v3 `SINGLE`은 `FINAL`, `SEMI + index`는 `SEMIFINAL_A/B`로 mapping한다.
- champion alias가 Data Dragon slug로 하나만 결정되지 않으면 임의 추측하지 않고 quarantine한다.
- v3 `admin_audit_log`는 폐기하지 않는다. `operator_id -> principal_id`, `principal_type=USER`,
  action/target/note/created_at을 보존하고 `result=APPLIED`, checkpoint/request는 NULL로 옮긴다.
  알려진 numeric target ID는 `legacy_entity_maps`의 새 UUID로 바꾸고 unknown target text는 그대로
  보존한다. payload는 JSON parse, size 제한, secret-key redaction을 통과한 값만
  `after_summary.legacyPayload`에 넣는다. 실패한 payload 원문은 runtime audit row에 넣지 않고
  staging/quarantine에 보존한다. 과거 reset/delete action을 보존해도 동일 command를 신규 API에
  제공한다는 뜻은 아니다.
- v3 Riot account는 import 설정의 legacy platform/routing pair를 사용한다. 기존 KR deployment의
  기본은 `KR`/`ASIA`이며 tag line만 보고 region을 추측하지 않는다.
- `activity_instance_id`, `activity_started_at`, screening/Riot cache는 이전하지 않는다.
- Discord channel/message/end-card ID는 운영 추적을 위해 보존한다.
- `guild_kv`를 PostgreSQL 범용 fallback table로 옮기지 않는다. 위에서 명시한
  `auction-lineup:*`만 domain row로 정규화하고 cache/screening/unknown key는 report 후 폐기한다.

### Atomic import unit과 quarantine

부분 child만 건너뛰지 않는다. import transaction과 quarantine 경계는 다음처럼 고정한다.

```text
USER_ACCOUNT    user별 Riot accounts 전체
RECRUITMENT     child series가 없는 일반 recruitment + participant/entry draft
SERIES          source recruitment + series + participant + 모든 game/input/MMR child
AUCTION_SETUP   child tournament가 없는 auction recruitment + participant/setup draft
TOURNAMENT      source recruitment + tournament + team/candidate/lot/action/match/game/MMR child
```

한 unit은 하나의 PostgreSQL transaction으로 적재한다. child pick/ban/MMR/lineup/FK가 하나라도
ERROR이면 transaction 전체를 rollback하고 unit root와 모든 원본 child를 quarantine한다. parent만
넣어 COMPLETED score나 active resume projection을 깨뜨리지 않는다. dependency user/season은 먼저
검증하며 없는 dependency도 unit ERROR다.

`REMEDIATED` unit만 동일 mapping으로 다시 실행한다. 완료/취소된 historical unit은 운영자의 사유
기록과 함께 `ACCEPTED_OMISSION`할 수 있지만 records 누락 count에 포함한다. active recruitment,
series, setup, tournament/match는 omission을 허용하지 않는다. 변환 불가능한 transient draft만
aggregate 원장을 유지한 채 `ACCEPTED_REENTRY`로 승인할 수 있으며 UI에서 재입력이 필요하다는
cutover checklist를 남긴다. soft-deleted hierarchy의 의도적 제외는 앞의 별도 omission report를
따르며 quarantine unit으로 되살리지 않는다.

### Cutover 검증

```text
source/target table별 row count
legacy soft-deleted hierarchy included=0, omitted count/checksum report
partial aggregate import count = 0
active aggregate OPEN/ACCEPTED_OMISSION quarantine count = 0
all OPEN ERROR quarantine count = 0
accepted historical omission/reentry count와 operator note 승인
orphan FK count = 0
cross-guild reference count = 0
Discord member/role sync 완료와 active operator count >= 1
중복 active season/lot/roster slot count = 0
일반/경매 game별 참가자 10명과 양 팀 5개 lane 검증
imported live game의 MMR ledger completeness와 before/after chain
live ledger + MIGRATION_SEED와 source current summary reconciliation
auction point ledger와 current_points reconciliation
series/tournament/match/game winner projection 비교
champion/queue/lineup quarantine report 승인
대표 records query shadow comparison
guild_counters.next_value = counter별
  GREATEST(target max display_number, 모든 source/omitted/quarantined ID,
           관련 sqlite_sequence.seq, 0) + 1
```

실제 cutover 전까지 v3 source는 그대로 운영한다. import rehearsal은 반복 가능한 새 staging DB에서
실행하며 source에 migration/write를 수행하지 않는다.

## 지원하지 않는 조회 의미

Records는 별도 `change`를 반환하지 않고 명시 season 또는 ACTIVE season만 조회한다. `전체 시즌` current
MMR을 합성하지 않는다. 이는 미정 계산식이 아니라 현재 API의 비지원 범위다.

일반 game 입력은 `/series/{id}/pick-ban`, 완료 원장 조회는 `/series/{id}/result`가 담당한다.
MVP에는 별도 최종 확인 command를 만들지 않으며 operations의 `RESULT` 작업은 입력 route로 연결한다.

## Schema 완료 조건

- 빈 DB에 repository의 모든 forward migration이 한 번에 성공한다.
- migration 순서에 존재하지 않는 parent FK가 없다.
- 모든 guild-owned FK가 tenant를 포함한다.
- 모든 frontend command가 명확한 aggregate, revision, transaction을 가진다.
- 10인/20인 경매 준비, 입찰, bracket, lineup, result를 모두 복원할 수 있다.
- 일반/경매 game submit과 MMR update가 원자적이다.
- 수동 MMR과 game/manual reversal을 append-only ledger로 설명할 수 있다.
- 두 concurrent 낙찰 또는 game submit 중 하나만 성공한다.
- Valkey가 없어도 committed domain state가 손실되지 않는다.
- Riot records cache hit와 recent page 재조회는 provider를 호출하지 않고, 30초 refresh cooldown이
  process restart와 concurrent request 사이에도 유지된다.
- runtime role로 audit/MMR/point/action ledger를 UPDATE/DELETE할 수 없다.
- v3 import rehearsal이 source/target reconciliation report를 생성한다.
- 대표 records/leaderboard/admin query의 tenant isolation과 index plan을 integration test로 검증한다.
