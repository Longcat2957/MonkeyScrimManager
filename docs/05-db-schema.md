# MonkeyBot DB Structure And Flow

작성일: 2026-07-10

## 기준

기존 기준:

- `../mookbot_v3/packages/core/src/db/schema.sql`

이번 프로젝트의 DB는 기존 schema를 그대로 복사하지 않는다. 기존 schema는 단일 Discord 서버를 위한 MVP에 가깝다. 새 프로젝트는 여러 Discord guild가 각자 독립적으로 내전 전적, MMR, 시즌, 경매, 운영 설정을 관리하는 구조여야 한다.

이 문서는 컬럼 설명서가 아니다. 전체 구조와 데이터 흐름을 한눈에 보기 위한 기준 문서다.

## 최상위 구조

```txt
Global
  ├─ users                    # Discord user identity
  ├─ riot_accounts            # Discord user에 연결된 Riot 계정
  ├─ user_guild_preferences   # 사용자별 guild 선택/정렬 선호
  └─ user_privacy_settings    # 사용자 외부 공개 범위

Guild
  └─ guilds                   # Discord server identity / 설치 상태
      ├─ guild_members         # guild 안에서의 user 상태
      ├─ guild_operator_roles  # guild별 운영자 역할
      ├─ guild_settings        # guild별 설정
      ├─ recruitment_entry_drafts # 엔트리 편성 draft와 revision
      ├─ series_pickban_drafts # 픽밴 draft와 revision
      ├─ seasons               # guild별 시즌
      ├─ recruitments          # guild별 일반 모집
      ├─ series                # guild별 일반 내전
      ├─ auction_recruitments  # guild별 경매 모집
      ├─ auction_tournaments   # guild별 경매 행사
      ├─ games                  # guild별 전적 원장
      ├─ revert_checkpoints     # guild별 고정 단계 되돌리기 기준점
      └─ admin_audit_log       # guild별 운영 감사 로그
```

핵심:

```txt
Discord OAuth2 = 누가 접속했는가
guild_id       = 어느 서버의 데이터를 보는가
role/settings  = 그 서버에서 무엇을 할 수 있는가
```

## Bot 설치부터 운영까지

```txt
1. 길드 운영자가 Discord bot 설치
   guilds 생성 또는 복구
   guilds.status = INSTALLED

2. 운영자가 /초기설정 guild_name admin_role_name 실행
   guild_members에 실행자 upsert
   실행자를 초기 owner/operator로 기록
   guild_settings.name = guild_name
   guild_operator_roles.role_name = admin_role_name
   기본 시즌 생성
   guilds.status = ACTIVE

3. Bot이 대시보드 링크 제공
   /g/{guildId}

4. 운영자가 Discord OAuth2로 웹 로그인
   users 확인
   guild_members 확인
   guild 권한 확인

5. guild dashboard 사용
   모집 / 일반 내전 / 경매 / 전적 / 관리자 로그가 모두 guild_id로 분리됨
```

guild lifecycle:

```txt
INSTALLED
  -> SETUP_REQUIRED
  -> ACTIVE
  -> DISABLED
  -> LEFT
```

초기에는 `INSTALLED`, `ACTIVE`, `LEFT`만 구현해도 충분하다. `SETUP_REQUIRED`, `DISABLED`는 상태 표현과 향후 운영 편의를 위한 확장 상태다.

## 전체 관계도

```txt
users
  ├─ riot_accounts
  ├─ user_guild_preferences
  ├─ user_privacy_settings
  └─ guild_members
        └─ guilds
             ├─ guild_operator_roles
             ├─ guild_settings
             ├─ seasons
             │    ├─ recruitments ── converts to ── series
             │    │                                 ├─ series_participants
             │    │                                 └─ games
             │    │                                      ├─ game_stats
             │    │                                      ├─ game_picks
             │    │                                      ├─ game_bans
             │    │                                      └─ mmr_changes
             │    │
             │    └─ auction_recruitments ── converts to ── auction_tournaments
             │                                             ├─ auction_teams
             │                                             │    └─ auction_team_members
             │                                             ├─ auction_bids
             │                                             └─ auction_matches
             │                                                  └─ games
             │                                                       ├─ game_stats
             │                                                       ├─ game_picks
             │                                                       └─ game_bans
             │
             ├─ user_lane_mmr
             ├─ user_champion_preferences
             ├─ record search reads games / stats / picks / bans
             ├─ recruitment_entry_drafts
             ├─ series_pickban_drafts
             ├─ revert_checkpoints
             └─ admin_audit_log
```

## Global 테이블

```txt
users
  discord_id
  display_name
  deleted_at

user_privacy_settings
  user_id -> users.discord_id
  public_profile_enabled
  show_riot_ids
  allow_external_record_search
  updated_at

user_guild_preferences
  user_id -> users.discord_id
  guild_id -> guilds.guild_id
  is_favorite
  sort_order
  last_selected_at

riot_accounts
  puuid
  user_id -> users.discord_id
  game_name
  tag_line
  is_main
  profile_icon_id
  main_position
```

원칙:

- `users.discord_id`는 전역 사용자 ID다.
- 인증은 Discord OAuth2만 사용한다.
- Riot 계정은 전역 사용자 프로필에 연결한다.
- 같은 Discord 사용자가 여러 guild에 있어도 Riot 계정 연결은 재사용할 수 있다.
- 개인정보 설정은 외부 공개 범위만 제어하며 같은 guild 내부의 경기 기록과 MMR을 숨기지 않는다.
- guild별 MMR과 기록은 절대 global user에 직접 붙이지 않는다.
- 사용자가 소속된 guild 중 어떤 guild를 먼저 볼지는 `user_guild_preferences`에서 관리한다.
- `user_guild_preferences`는 데이터 소유권이 아니라 사용자별 진입/정렬/즐겨찾기 설정이다.

## Guild 루트 테이블

```txt
guilds
  guild_id              # Discord guild id
  name
  icon_url
  is_externally_public  # guild 외부 공개 허용 여부
  status                # INSTALLED | SETUP_REQUIRED | ACTIVE | DISABLED | LEFT
  installed_at
  activated_at
  left_at
  created_by

guild_members
  guild_id
  user_id -> users.discord_id
  display_name
  joined_at
  last_seen_at
  deleted_at

guild_operator_roles
  guild_id
  role_id               # Discord role id
  role_name
  created_by

guild_settings
  guild_id
  key
  value
```

역할:

- `guilds`는 multi-tenant의 루트다.
- `guilds.is_externally_public`은 guild 외부 사용자가 guild 공개 화면이나 공개 전적을 볼 수 있는지 결정한다.
- `guild_members`는 전역 user가 특정 guild에 속하는지 표현한다.
- `guild_operator_roles`는 서버마다 다른 운영자 역할을 지원한다.
- `guild_settings`는 모집 채널, 로그 채널, 기본 시즌, 기타 서버 설정을 담는다.

중요:

- 모든 운영 데이터는 `guild_id`로 시작해야 한다.
- 웹 URL의 `/g/{guildId}`와 DB의 `guild_id`가 같은 경계를 만든다.
- Bot command는 항상 `interaction.guildId`를 기준으로 API를 호출한다.
- `is_externally_public = false`인 guild의 내부 전적, 멤버, 운영 정보는 guild 소속 사용자에게만 노출한다.
- 외부 공개가 켜져도 운영 화면, 감사 로그, 멤버 관리, 권한 정보는 공개 대상이 아니다.

## 일반 내전 구조

```txt
guilds
  └─ seasons
       └─ recruitments
            ├─ recruitment_participants
            ├─ recruitment_participant_roles
            └─ converts to series
                 ├─ series_participants
                 └─ games
                      ├─ game_stats
                      ├─ game_picks
                      ├─ game_bans
                      └─ mmr_changes
```

주요 테이블:

```txt
seasons
  guild_id

recruitments
  guild_id
  season_id
  status
  converted_series_id
  channel_id
  message_id

series
  guild_id
  season_id
  status
  winning_team
  deleted_at

user_lane_mmr
  guild_id
  user_id
  season_id
  role
  mmr

mmr_changes
  guild_id
  game_id
  user_id
  season_id
  role
  delta
```

흐름:

```txt
/내전모집
  -> recruitments(guild_id) 생성

Discord 참가/취소/라인선호
  -> recruitment_participants
  -> recruitment_participant_roles

엔트리 편성
  -> recruitment_entry_drafts(guild_id, recruitment_id, revision)

시리즈 생성
  -> recruitments.status = CONVERTED
  -> series(guild_id) 생성
  -> series_participants 생성

게임 결과 입력
  -> games(guild_id, ranked_series_id)
  -> game_stats / game_picks / game_bans
  -> user_lane_mmr(guild_id, user_id, season_id, role) 갱신
  -> mmr_changes(guild_id) 생성
```

핵심 invariant:

```txt
recruitments.guild_id == series.guild_id
series.guild_id == games.guild_id
user_lane_mmr는 guild_id별로 완전히 분리
```

기존 v3 invariant:

```txt
series.id == recruitments.id
```

새 프로젝트에서는 이 규칙을 유지할 수 있지만, multi-guild 구조에서는 더 중요한 규칙이 `guild_id` 격리다. 내부 ID는 전역 autoincrement를 유지하고, 필요하면 나중에 guild별 표시 번호를 추가한다.

```txt
id          # global internal id
guild_id    # tenant boundary
guild_seq   # optional display number, later
```

## 경매 내전 구조

```txt
guilds
  └─ seasons
       └─ auction_recruitments
            ├─ auction_recruitment_participants
            └─ converts to auction_tournaments
                 ├─ auction_teams
                 │    └─ auction_team_members
                 ├─ auction_bids
                 └─ auction_matches
                      ├─ auction_match_lineups
                      └─ games
                           ├─ game_stats
                           ├─ game_picks
                           └─ game_bans
```

주요 테이블:

```txt
auction_recruitments
  guild_id
  season_id
  target_count
  status
  converted_tournament_id

auction_tournaments
  guild_id
  season_id
  status
  format
  current_bid_target_user_id
  bid_candidate_queue_user_ids
  champion_team_id
  deleted_at

auction_teams
  guild_id
  tournament_id
  captain_user_id
  initial_points
  current_points

auction_team_members
  guild_id
  team_id
  user_id
  acquired_via

auction_bids
  guild_id
  tournament_id
  target_user_id
  team_id
  points

auction_matches
  guild_id
  tournament_id
  team1_id
  team2_id
  status

auction_match_lineups
  guild_id
  match_id
  team_id
  user_id
  role                  # TOP | JUNGLE | MID | ADC | SUPPORT
```

흐름:

```txt
/경매모집
  -> auction_recruitments(guild_id) 생성

Discord 참가/취소
  -> auction_recruitment_participants

경매 시작
  -> auction_recruitments.status = CONVERTED
  -> auction_tournaments(guild_id) 생성

팀장/포인트/입찰
  -> auction_teams
  -> auction_tournaments.current_bid_target_user_id
  -> auction_tournaments.bid_candidate_queue_user_ids
  -> auction_bids
  -> auction_team_members

브래킷 생성
  -> auction_matches

경기 시작 전 라인업 확정
  -> auction_match_lineups
  -> 각 팀 TOP/JUNGLE/MID/ADC/SUPPORT 5명 확정

경매 게임 결과 입력
  -> games(guild_id, auction_match_id)
  -> game_stats / game_picks / game_bans
  -> MMR 변경 없음
```

핵심 invariant:

```txt
auction_recruitments.guild_id == auction_tournaments.guild_id
auction_tournaments.guild_id == auction_matches.guild_id
auction_matches.guild_id == auction_match_lineups.guild_id
auction_matches.guild_id == games.guild_id
경매 match는 양 팀 라인업 5명씩 확정되기 전 game 결과 입력 불가
경매 games는 user_lane_mmr/mmr_changes를 건드리지 않음
```

기존 v3 invariant:

```txt
auction_tournaments.id == auction_recruitments.id
```

이 규칙도 유지 가능하지만, 반드시 `guild_id`와 함께 검증한다.

## 게임 원장

`games`는 일반 내전과 경매 내전이 함께 쓰는 공통 게임 원장이다.

```txt
games
  guild_id
  ranked_series_id
  auction_match_id
  game_number
  winning_team
  team1_side
```

분기:

```txt
일반 내전 game
  guild_id = series.guild_id
  ranked_series_id = series.id
  auction_match_id = NULL
  MMR 반영

경매 내전 game
  guild_id = auction_matches.guild_id
  ranked_series_id = NULL
  auction_match_id = auction_matches.id
  MMR 반영
```

하위 기록:

```txt
games
  ├─ game_stats
  ├─ game_picks
  └─ game_bans
```

원칙:

- game 하위 테이블에도 `guild_id`를 둘지 여부는 구현 시 선택할 수 있다.
- 하지만 query boundary는 항상 `games.guild_id`를 통해 검증해야 한다.
- 실수 방지를 위해 하위 테이블에도 `guild_id`를 중복 저장하는 방식을 선호한다.

## 전적 검색 구조

전적 검색은 새 원장을 만들지 않는다. `games`를 기준으로 일반 내전과 경매 내전 기록을 guild 안에서 읽는다.

```txt
/g/{guildId}/records
  -> users / guild_members / riot_accounts
  -> series_participants 또는 auction_team_members
  -> games
       ├─ game_stats
       ├─ game_picks
       ├─ game_bans
       └─ mmr_changes
  -> user_lane_mmr
```

검색 대상:

```txt
Discord user
Riot ID
guild display name
champion
series / auction tournament
```

두 종류의 전적을 구분한다.

```txt
내부 내전 전적
  guild_id로 격리
  games 원장이 source of truth
  MMR, 라인, 승패, pick/ban, 참가 이력 조회

Riot 외부 전적
  Riot API 조회 결과
  내부 MMR의 source of truth가 아님
  screening/report/cache 용도
```

Riot 외부 전적 캐시:

```txt
Valkey key = guild:{guildId}:riot-match:{region}:{puuid}:{matchId}
Valkey key = guild:{guildId}:riot-summary:{region}:{puuid}
```

원칙:

- 일반적인 전적검색 UI처럼 Riot ID로 검색할 수 있어야 한다.
- 검색 결과는 먼저 해당 guild의 내부 내전 기록을 보여준다.
- Riot API 기반 최근 게임/랭크 정보는 보조 패널로 둔다.
- 외부 Riot match 전체를 1차 DB 원장으로 저장하지 않는다.
- 나중에 필요하면 `riot_match_cache`, `riot_match_participants`를 별도 read-model로 추가한다.

## MMR 격리

기존 단일 서버 구조:

```txt
user_lane_mmr(user_id, season_id, role)
```

새 multi-guild 구조:

```txt
user_lane_mmr(guild_id, user_id, season_id, role)
```

같은 사용자의 MMR은 guild마다 다르다.

```txt
Discord user 123
  ├─ Guild A / Season 1 / TOP = 1620
  ├─ Guild B / Season 1 / TOP = 1480
  └─ Guild C / Season 3 / MID = 1550
```

MMR 변경 기록도 guild별로 분리한다.

```txt
mmr_changes(guild_id, game_id, user_id, season_id, role, delta)
```

## 실시간 편집 상태 / Valkey

상태 저장 경계:

- `recruitment_entry_drafts`, `series_pickban_drafts`, 경매 단계/입찰/배치/라인업/결과는 DB에 저장한다.
- 동시 편집 aggregate는 `revision`, `updated_by`, `updated_at`을 가진다.
- Valkey에는 WebSocket Pub/Sub, presence, 짧은 보조 lock, 재생성 가능한 cache만 둔다.
- `guild_kv` 범용 테이블과 DB/Valkey 이중 write는 새 구조에 만들지 않는다.

revision schema, mutation 단위, Valkey key, 충돌과 장애 복구 기준은 [Realtime Collaboration](07-realtime-collaboration.md)을 따른다.

## 되돌리기 / Revert Checkpoint

사람은 실수한다. 하지만 여기서 말하는 되돌리기는 범용 undo/redo 시스템이 아니다. 정해진 운영 단계에서 이전 상태로 돌아가는 기능이다.

```txt
revert_checkpoints
  guild_id
  actor_id
  action
  target_type
  target_id
  checkpoint_state
  status                # ACTIVE | REVERTED
  created_at
  reverted_at
```

전체 구조:

```txt
operator action
  -> permission check
  -> 필요 시 revert checkpoint 기록
  -> domain mutation
  -> admin_audit_log 기록
  -> WebSocket invalidate

revert action
  -> 고정된 target의 checkpoint 조회
  -> 해당 단계로 복구
  -> checkpoint.status = REVERTED
  -> admin_audit_log 기록
```

되돌리기 지원 대상:

```txt
일반 내전
  엔트리 편성 되돌리기
  마지막 game 삭제

경매 내전
  최근 입찰 단계 되돌리기
  수동 배치 되돌리기
  match lineup 확정 취소
  마지막 game 삭제
```

원칙:

- 삭제보다 상태 전이와 soft-delete를 우선한다.
- 일반화된 multi-step undo/redo를 만들지 않는다.
- 되돌리기는 화면에서 명확히 노출된 고정 작업에만 제공한다.
- MMR 되돌리기는 마지막 game 삭제처럼 명확한 작업에 한정한다.
- 경매 입찰 되돌리기는 최근 입찰 단계 복구에 한정한다.
- 라인업 확정 되돌리기는 game result 입력 전까지만 허용한다.
- 복구가 복잡한 destructive 작업은 되돌리기보다 soft-delete와 확인 절차로 처리한다.

## Audit 격리

```txt
admin_audit_log
  guild_id
  operator_id
  action
  target_type
  target_id
  checkpoint_id
  payload
  note
  created_at
```

기록해야 하는 작업:

- 초기 설정
- 운영자 역할 변경
- 강제 삭제
- MMR 수동 수정
- 시즌 결과 리셋
- 오래된 내전 정리
- 유저 강제 삭제
- 게임 결과 되돌리기
- 경매 단계 되돌리기/취소

원칙:

- audit log는 guild별로 조회된다.
- global admin 기능이 생기더라도 guild target이 있는 작업은 항상 `guild_id`를 기록한다.

## Soft Delete

soft-delete 대상:

```txt
users.deleted_at               # global user 비활성
guild_members.deleted_at       # guild 안에서 유저 숨김/제거
series.deleted_at
auction_tournaments.deleted_at
auction_matches.deleted_at
```

guild lifecycle:

```txt
guilds.status = LEFT
guilds.left_at = timestamp
```

원칙:

- bot이 guild에서 제거되어도 즉시 운영 데이터를 삭제하지 않는다.
- 재설치 가능성을 위해 `guilds`와 guild-scoped records는 보존한다.
- 실제 purge는 별도 관리자 작업으로만 한다.

## Legacy

기존 v3에서 새 프로젝트가 새로 의존하지 않을 것:

```txt
series.activity_instance_id
series.activity_started_at
Discord Activity session/instance 개념
```

유지 가능:

```txt
series.channel_id
series.message_id
series.end_card_channel_id
series.end_card_message_id
recruitments.channel_id
recruitments.message_id
auction_recruitments.channel_id
auction_recruitments.message_id
```

원칙:

- Activity 필드는 신규 write 금지.
- Discord 모집 메시지/종료 카드 추적 필드는 guild-scoped 운영에 계속 필요할 수 있다.

## Migration 방향

가져올 것:

```txt
packages/core/src/db/schema.sql
packages/core/src/db/migrate.ts
packages/core/src/db/* repository
packages/core/src/cloudflare/d1.ts
packages/core/src/kv/*
DB test harness
```

하지만 그대로 쓰지 않고 다음 구조 변경을 먼저 반영한다.

필수 추가:

```txt
guilds
guild_members
guild_operator_roles
guild_settings
guild_id on seasons
guild_id on recruitments / series / games
guild_id on auction_recruitments / auction_tournaments / auction_matches
guild_id on user_lane_mmr / mmr_changes
guild_id on admin_audit_log
recruitment_entry_drafts / series_pickban_drafts with revision
```

v3 `guild_kv` migration:

- `entry:*`, `pickban:*` 중 진행 중 aggregate의 최신 값만 새 draft 테이블로 이전한다.
- Riot/screening cache는 이전하지 않고 Valkey에서 다시 생성한다.
- 기타 key는 도메인 소유 테이블을 정한 뒤 이전하며 범용 `guild_kv` fallback을 남기지 않는다.

금지:

```txt
DROP TABLE
broad DELETE
ALTER TABLE ... DROP COLUMN
ALTER TABLE ... RENAME
TRUNCATE
```

검증:

```txt
Guild A와 Guild B의 MMR이 섞이지 않음
Guild A와 Guild B의 season이 독립적임
Guild A와 Guild B의 recruitment/series id 조회가 guild_id로 격리됨
Bot 재설치 시 guild 데이터가 복구 가능함
일반 game 결과는 해당 guild MMR만 갱신
경매 game 결과도 해당 guild MMR만 갱신
모든 admin audit log는 guild_id를 가진다
```
