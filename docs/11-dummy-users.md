# Development Dummy Users

## 목적

더미 사용자는 테스트 코드에만 존재하는 fixture가 아니라 development DB에 지속되는 서비스 사용자다. 실제
Discord 계정 없이도 길드 회원, 모집 참가자, MMR 대상, 전적 참가자 등 기존 도메인 관계에 참여한다. 신규
기능은 더미 전용 상태를 만들지 않고 공통 `UserId`와 기존 도메인 테이블을 사용한다.

## 신원 경계

- `users.user_id`는 provider와 무관한 내부 식별자다.
- 실제 사용자는 `identity_kind=DISCORD`이고 `user_discord_identities`에 Discord snowflake를 가진다.
- 더미 사용자는 `identity_kind=DUMMY`이고 `dmy_<uuid-v7>` 형식의 ID를 가진다.
- 더미 사용자는 OAuth session, Discord role, mention, DM, REST/Gateway 동기화 대상이 될 수 없다.
- Discord 입력 경계에서만 `DiscordUserId`를 사용하고 저장된 사용자 참조에는 `UserId`를 사용한다.

`UserId`는 client가 형식에 따라 권한을 판단하는 discriminator가 아니다. API가 함께 반환하는
`identityKind=DISCORD|DUMMY`를 표시 용도로만 사용하고, 실제 권한과 기능 자격은 session principal과
길드 membership에서 판단한다. `GuildAdminMember.id`, 모집 참가자 ID, 랭킹 player ID 같은 공통 DTO도
opaque `UserId`를 반환한다.

## 데이터 소유권

`dummy_user_profiles`는 stable key, 보관 상태, avatar seed, 메모와 변경 이력만 보관한다. 실제 기능 상태는
다음 기존 테이블이 authoritative하다.

| 상태                            | 저장 위치                     |
| ------------------------------- | ----------------------------- |
| 길드 닉네임, 활성 상태, 주 라인 | `guild_members`               |
| 시즌별 라인 MMR                 | `user_lane_mmr`               |
| 선호 챔피언                     | `user_champion_preferences`   |
| 개인정보 공개 범위              | `user_privacy_settings`       |
| 모집, 시리즈, 경매, 경기 참가   | 각 workflow participant table |
| 가상 Riot 계정                  | `riot_accounts(origin=DUMMY)` |

더미 프로필 JSON에 기능 상태를 복제하지 않는다. 가상 Riot 계정은 실제 Riot 검색, 갱신, 자동완성 대상에서
제외하고 길드 내부 UI projection에서만 사용한다.

`riot_accounts`는 origin에서 계산한 owner identity kind와 `users(user_id, identity_kind)` composite FK로
다음 불변식을 강제한다.

```text
identity_kind=DISCORD <-> origin=RIOT
identity_kind=DUMMY   <-> origin=DUMMY
```

따라서 더미 관리 API가 실제 Riot PUUID row를 가져가거나 OAuth 사용자가 합성 계정을 소유하는 상태는
application 실수만으로도 저장할 수 없다.

## 관리 화면

전역 관리 화면은 `/dev/users`다. 한 사용자를 여러 길드와 기능에서 재사용할 수 있으므로 특정 길드 관리
페이지에 소유권을 두지 않는다.

- 단일 또는 일괄 생성
- 라인과 초기 MMR을 포함한 길드 배치
- Privacy, 선호 챔피언, 가상 Riot 계정 수정
- 5인, 10인, 20인 재사용 그룹 관리
- 활성/보관 전환
- 기존 모집에 참가자 일괄 배치

길드 관리 화면과 기능 화면은 공통 더미 회원 선택기를 호출한다. 선택기는 대상 길드의 활성 더미 회원만
보여주고 실제 application service가 정원, 중복 참가, 상태 전이를 검증한다.

## HTTP 계약

라우터는 `MONKEYBOT_DUMMY_USERS_ENABLED=true`일 때만 등록되며 production 환경은 해당 설정 자체를
거절한다. 기능이 비활성인 서버에서는 `/api/dev/**`가 `404`다.

| 경로                                                                    | 책임                                   |
| ----------------------------------------------------------------------- | -------------------------------------- |
| `GET/POST /api/dev/dummy-users`                                         | 검색/목록, 단일 생성                   |
| `POST /api/dev/dummy-users/bulk`                                        | 최대 100명 원자적 일괄 생성            |
| `GET/PATCH/DELETE /api/dev/dummy-users/{userId}`                        | 상세, revision 수정, 영향 확인 후 삭제 |
| `GET /api/dev/dummy-users/{userId}/deletion-impact`                     | 삭제 차단/정리 가능 참조 count         |
| `POST /api/dev/dummy-users/{userId}/archive`                            | 보관                                   |
| `POST /api/dev/dummy-users/{userId}/restore`                            | 복원                                   |
| `GET /api/dev/dummy-users/catalog`                                      | 배치 가능한 guild/active season 조회   |
| `PUT/DELETE /api/dev/dummy-users/{userId}/guilds/{guildId}`             | 길드 소속과 초기 MMR 관리              |
| `PUT /api/dev/dummy-users/{userId}/champion-preferences/{lane}`         | lane별 선호 챔피언 전체 교체           |
| `PUT/DELETE /api/dev/dummy-users/{userId}/riot-account`                 | 합성 Riot projection 관리              |
| `GET/POST /api/dev/dummy-groups`                                        | 재사용 group 목록/생성                 |
| `PATCH/DELETE /api/dev/dummy-groups/{groupId}`                          | group metadata 수정/삭제               |
| `PUT /api/dev/dummy-groups/{groupId}/members`                           | 순서 있는 member 전체 교체             |
| `GET /api/dev/guilds/{guildId}/recruitments/{id}/participants`          | 모집과 더미 후보 snapshot              |
| `POST /api/dev/guilds/{guildId}/recruitments/{id}/participants/preview` | 비변경 validation preview              |
| `POST/DELETE /api/dev/guilds/{guildId}/recruitments/{id}/participants`  | 더미 참가자 원자적 추가/취소           |

전체 request/response schema와 오류 status는 [OpenAPI](../openapi/monkeybot.yaml), 업무 설명은
[Backend Routing](routing/backend.md)을 따른다.

## 권한과 감사

- `MONKEYBOT_DUMMY_USERS_ENABLED=true`인 local/development/test 환경에서만 route를 활성화한다.
- production 환경에서는 설정 자체를 거절한다.
- 읽기와 쓰기는 로그인한 service admin만 수행한다.
- mutation은 CSRF, idempotency/revision 검증과 request ID를 사용한다.
- mutation의 `Idempotency-Key`는 visible ASCII 1..128 byte이고 24시간 동안 같은 요청을 replay한다.
- 감사 로그의 actor는 실제 로그인 사용자이고 target은 더미 사용자 또는 그룹이다.
- 더미 사용자는 운영 권한의 security principal로 사용하지 않는다.

모집 참가자 관리에는 service admin만으로 충분하지 않다. 현재 guild의 active operator membership도
필요하다. Preview는 모든 문제를 `issues[]`로 보여주지만 apply는 row lock 아래에서 같은 규칙을 다시
검증한다. Apply 성공 시 participant, main-lane 기반 preferred lane, 모집/entry draft revision, audit,
outbox invalidate가 한 transaction이다.

## 생명주기

기능 이력이 없는 사용자는 명시적 영향 확인 후 삭제할 수 있다. 삭제 전 API는 guild membership, 일반/경매
모집 참가, game 참가, session, Discord identity, 실제 Riot account를 blocking reference로 계산한다.
Group membership, 선호 챔피언, 합성 Riot account는 삭제 transaction에서 정리 가능한 reference다.

Blocking reference가 하나라도 있으면 `409 dummy_user_deletion_blocked`이고 `ARCHIVED`로 전환한다.
보관 사용자는 과거 조회에는 남지만 수정, 신규 길드 배치, group 추가, 모집 참가자 선택에서는 제외한다.
길드 소속만 제거할 때도 보존할 workflow/MMR/audit history가 있으면 삭제하지 않고 `INACTIVE`로 둔다.

Archive는 계정 삭제와 별도 상태 전이다. `users.deleted_at`을 설정하지 않으며, `OPEN|CLOSED` 일반 모집
또는 경매 모집에 `JOINED` 참가가 남아 있으면 `409 dummy_user_archive_blocked_by_recruitment`로 전체
변경을 거절한다. 응답의 `details.joinedRecruitments`와 `details.joinedAuctionRecruitments`로 먼저 취소할
참가 유형을 확인한다.

Archive 성공 시 현재 `ACTIVE`인 모든 `guild_members`만 `INACTIVE`로 전환한다. 영향받은 각 길드에서
`guild_admin_state[MEMBERS]` revision과 `MEMBER_STATUS_CHANGED` 감사 로그를 같은 transaction으로
갱신한다. Admin members 화면은 원래 WebSocket topic이 없으므로 이 전이만을 위한 outbox도 만들지 않는다.
Restore는 더미 profile만 `ACTIVE`로 복원하며 길드 소속을 자동 재활성화하지 않는다. 과거에 이미
비활성이던 소속을 잘못 되돌리지 않기 위해 필요한 길드만 관리자가 명시적으로 `ACTIVE`로 바꾼다.

## 신규 기능 연결 규칙

1. 참가자와 대상 사용자를 `UserId`로 받는다.
2. 길드 자격은 `guild_members`에서 검증한다.
3. 더미 여부로 계산, 권한, 상태 전이를 분기하지 않는다.
4. Discord/Riot 외부 호출 직전에만 identity/origin을 확인한다.
5. 관리 UI는 공통 더미 회원 선택기를 재사용한다.
6. 기능별 데이터는 해당 기능의 정상 application service를 통해 기록한다.
