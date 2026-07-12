# Bot Routing

작성일: 2026-07-11

## 범위

이 문서는 `apps/bot`의 Discord slash command, button/select interaction, 웹 딥링크 생성 규칙을 다룬다.

Bot routing은 HTTP URL routing이 아니라 Discord interaction routing이다. 새 프로젝트에서는 command handler와 component custom id handler를 명확히 분리한다.

Bot route의 책임:

- Discord slash command 등록.
- command option parsing.
- Discord permission/member context 확인.
- 모집 메시지 렌더링.
- button/select interaction 처리.
- 웹 딥링크 생성.
- API -> Bot internal notify 수신 후 Discord 알림 발행.

Bot route의 비책임:

- 복잡한 도메인 상태 전이의 최종 판단.
- MMR 계산.
- audit log 직접 조작.
- 웹 화면 rendering.

인증 정책:

- Bot은 별도 앱 회원가입을 만들지 않는다.
- 사용자는 Discord interaction의 Discord user id로 식별한다.
- `/등록`은 앱 회원가입이 아니라, Discord user id에 Riot ID를 연결하는 명령이다.
- 웹 콘솔 인증은 Discord OAuth2만 사용한다.

## 사용자 Slash Commands

### `/도움말`

목적:

- 사용자/내전/경매/운영자 도움말 표시.

Backend:

- 필요 없음 또는 최소 상태 확인.

### `/초기설정 [길드이름] 운영자역할`

목적:

- bot이 설치된 Discord guild를 MonkeyBot 운영 공간으로 활성화한다.
- 선택한 Discord role ID를 관리 권한의 source로 저장한다.
- 길드 이름을 생략하면 현재 Discord guild 이름을 사용한다.
- 설정 완료 후 guild dashboard link를 제공한다.

Backend:

- guild setup API/service.

권한:

- Discord guild 관리자 또는 bot 설치 권한자.

Web link:

- `/g/{guildId}`.

### `/등록 riot_id`

목적:

- Discord user id 기준 사용자 record 확인/upsert 및 Riot ID 연결.
- 앱 회원가입이 아니라 Riot 프로필 연결이다.

Backend:

- user/Riot account 등록 API 또는 core service.

### `/내정보 [user]`

목적:

- 등록 정보, Riot 계정, 시즌 라인별 MMR 조회.

Backend:

- global user profile API.
- 현재 guild의 records search API를 같은 Discord user ID로 조회해 라인별 MMR을 구성한다.

Web link:

- `/users/{discordId}`
- `/g/{guildId}/records?q={discordId}`

### `/내정보갱신`

목적:

- Riot 소환사 아이콘 재동기화.

Backend:

- riot account refresh API.

### `/내전기록 [user]`

목적:

- 현재 guild 기준 라인별 통계, 최근 내전, MMR 변동 조회.

Backend:

- `GET /api/guilds/{guildId}/records/search?q={discordId}`.

Web link:

- `/users/{discordId}`
- `/g/{guildId}/records?q={discordId}`

### `/랭킹 [라인]`

목적:

- 라인별 시즌 MMR 상위권 조회.

Backend:

- leaderboard API.

Web link:

- `/g/{guildId}/leaderboard`

### `/전적 [riot_id]`

목적:

- Riot 외부 전적 검색.
- Riot profile, rank, champion aggregate, recent match를 조회한다.
- guild 내부 내전 기록은 섞지 않고 `/내전기록`에서 조회한다.

Backend:

- Riot lookup API/core service.

Web link:

- `/records?q={riotId}`

### `/전적검토 [riot_id|user]`

MVP command registry에 등록하지 않는다. Screening UI와 API 계약이 별도로 확정된 이후에만
추가한다.

현재 게임/Riot Spectator command는 MVP에서 제공하지 않는다.

## 일반 내전 운영 Commands

### `/내전모집`

목적:

- 5v5 일반 모집 생성.
- Discord 모집 메시지 발행.

처리 순서:

1. 공개 interaction을 즉시 defer한다.
2. interaction ID를 `Idempotency-Key`로 Axum create command에 전달한다.
3. response snapshot으로 모집 메시지를 게시한다.
4. 게시된 channel/message ID를 별도 idempotent bind command로 저장한다.

Bot은 guild counter, ACTIVE season, 참가자, audit, outbox를 직접 변경하지 않는다.

Backend:

- recruitment create API 또는 bot용 service endpoint.

Message components:

- 참석 button.
- 참석 취소 button.
- 선호 라인 multi-select.
- 정원 도달 시 operator action.

Web link:

- 정원 도달/마감 후 `/g/{guildId}/recruitments/{id}/entry`.

### `/내전인원추가 모집 멤버`

목적:

- 일반 모집 강제 참가.

Backend:

- recruitment member add API/service.

권한:

- operator.

### `/내전인원삭제 모집 멤버`

목적:

- 일반 모집 강제 제거.

Backend:

- recruitment member remove API/service.

권한:

- operator.

### `/내전랜덤인원추가 모집 인원 종류`

목적:

- 일반/경매 모집에 랜덤 인원 추가.

Backend:

- recruitment 또는 auction recruitment member add service.

권한:

- operator.

### `/내전모집삭제 모집`

목적:

- 일반 모집 물리/응급 삭제 또는 취소.

Backend:

- recruitment cancel/delete API/service.

권한:

- operator.

Audit:

- Backend에서 기록.

### `/내전모집복구 모집`

목적:

- 취소 모집을 OPEN으로 복구.

Backend:

- `POST /api/guilds/{guildId}/recruitments/:id/reopen`를 service principal로 호출한다.

권한:

- operator.

### `/내전목록 [상태] [시즌] [limit]`

목적:

- 시리즈 목록 조회.

Backend:

- `GET /api/guilds/{guildId}/series?state=active`
- `GET /api/guilds/{guildId}/series?state=completed`

Web link:

- `/g/{guildId}/series`
- `/g/{guildId}/series#completed`

### `/내전강제취소 series_id`

목적:

- 시리즈를 CANCELLED 처리한다. 확정 game/MMR이 있으면 마지막 game tail부터 명시적 revert를
  선행하며 원장을 물리 삭제하지 않는다.

Backend:

- series cancel/revert application service.

권한:

- operator/admin.

Audit:

- Backend에서 기록.

### `/내전조기종료 시리즈 결과`

목적:

- 시리즈 조기 완료/무효 처리.

Backend:

- series early complete/cancel API/service.

권한:

- operator.

## 경매 내전 운영 Commands

### `/경매모집 정원`

목적:

- 10인 또는 20인 경매 모집 생성.
- Discord 경매 모집 메시지 발행.

Backend:

- auction recruitment create API/service.

Message components:

- 참석 button.
- 참석 취소 button.
- 참가 lane select. guild main lane을 기본값으로 사용하며 미설정 사용자는 lane 선택 후에만 참가를
  확정한다.
- 정원 도달 시 operator action.

Web link:

- `/g/{guildId}/auction/recruitments/{id}/draft`.

### `/경매인원추가 모집 멤버 [라인]`

목적:

- 경매 모집 강제 참가.

Backend:

- `POST /api/guilds/{guildId}/auction-recruitments/:id/members`.

권한:

- operator.

### `/경매인원삭제 모집 멤버`

목적:

- 경매 모집 강제 제거.

Backend:

- `DELETE /api/guilds/{guildId}/auction-recruitments/:id/members/:uid`.

권한:

- operator.

### `/경매목록 [상태] [시즌] [limit]`

목적:

- 경매 토너먼트 목록 조회.

Backend:

- auction tournament list API.

Web link:

- `/g/{guildId}/auction`

### `/경매강제취소 모집`

목적:

- 경매 모집과 토너먼트를 CANCELLED 처리한다. lot/action/point/game 원장은 삭제하지 않는다.

Backend:

- auction cancel application service.

권한:

- operator/admin.

Audit:

- Backend에서 기록.

### `/경매조기종료 토너먼트`

목적:

- 진행 중 경매 토너먼트 취소.

Backend:

- `POST /api/guilds/{guildId}/auction-tournaments/:id/cancel`을 service principal로 호출한다.

권한:

- operator.

## 관리자 Commands

### `/일괄등록 [dry_run] [main_position]`

목적:

- 길드 멤버 일괄 등록.
- 별명 Riot ID 자동 연결.
- 선택적 주 포지션 갱신.

Backend:

- bulk register service.

권한:

- admin/operator.

### `/mmr수정 user role delta [note]`

목적:

- 라인 MMR 수동 조정.

Backend:

- MMR admin API/service.

권한:

- admin/operator.

Audit:

- Backend에서 기록.

### `/새시즌 name`

목적:

- 현재 ACTIVE 시즌을 CLOSED로 종료하고 새 ACTIVE 시즌을 생성한다.
- 과거 game/MMR/auction ledger를 삭제하거나 0으로 리셋하지 않는다.

Backend:

- `POST /api/guilds/{guildId}/seasons` season rotate application service.

권한:

- admin.

Audit:

- Backend에서 기록.

### `/오래된내전정리 [days]`

목적:

- 방치 모집/시리즈/경매를 조건에 따라 CANCELLED 처리한다. hard delete는 하지 않는다.

Backend:

- cleanup stale API/service.

권한:

- admin/operator.

### `/로그`

목적:

- audit log 웹뷰 링크 발급.

Web link:

- `/g/{guildId}/admin?tab=logs`.

### `/되돌리기 [target]`

목적:

- 현재 guild에서 최근 revert checkpoint를 되돌린다.
- target이 없으면 호출한 채널/최근 작업 기준으로 되돌린다.
- target이 있으면 series, auction tournament, auction match 등 범위를 좁힌다.
- 범용 undo/redo가 아니라 엔트리 편성, 마지막 게임, 경매 입찰 단계, 라인업 확정 취소 같은 고정 단계 복구다.

Backend:

- Bot이 target/채널/최근 작업 기준으로 ACTIVE checkpoint ID를 resolve한 뒤 canonical
  `POST /api/guilds/:guildId/reverts/{checkpointId}/apply` command를 호출한다.

권한:

- operator/admin.

Audit:

- Backend에서 기록.

Web link:

- `/g/{guildId}/admin?tab=operations`.

### `/유저비활성화 discord_id`

목적:

- 해당 guild member를 INACTIVE 처리한다. global user와 과거 원장은 삭제하지 않는다.

Backend:

- guild member status application service.

권한:

- admin.

Audit:

- Backend에서 기록.

## Component Interaction Routing

### 일반 모집 message components

Handlers:

- `recruit:join:{recruitmentId}`
- `recruit:leave:{recruitmentId}`
- `recruit:roles:{recruitmentId}`
- `recruit:start-entry:{recruitmentId}`
- `recruit:cancel:{recruitmentId}`
- `recruit:cancel-confirm:{recruitmentId}`

목적:

- 참석/취소.
- 선호 라인 선택.
- operator의 엔트리 편성 시작.
- 모집 취소 확인.

Web link:

- `recruit:start-entry` 처리 후 `/g/{guildId}/recruitments/{id}/entry`.

Button/select handler는 API 호출 전에 interaction을 defer하고, 성공 response의 전체 recruitment
snapshot으로 원본 메시지를 다시 렌더링한다. join/leave/roles는 OPEN 상태를, start-entry는 operator
role/OPEN/10명을 backend transaction에서 다시 검증한다.

### 경매 모집 message components

Handlers:

- `auction-recruit:join:{recruitmentId}`
- `auction-recruit:leave:{recruitmentId}`
- `auction-recruit:lane:{recruitmentId}`
- `auction-recruit:start:{recruitmentId}`
- `auction-recruit:cancel:{recruitmentId}`
- `auction-recruit:cancel-confirm:{recruitmentId}`

목적:

- 참석/취소.
- main lane이 없는 사용자의 lane 선택과 참가 확정.
- operator의 경매 시작.
- 모집 취소 확인.

Web link:

- `auction-recruit:start` 처리 후 `/g/{guildId}/auction/recruitments/{id}/draft`.

## API -> Bot Internal Routing

Bot은 Backend internal notify를 받아 Discord 메시지/알림을 best-effort로 발행한다.

Internal events:

- `recruit-refresh`
- `series-created`
- `series-completed`
- `auction-tournament-completed`
- `auction-match-created`
- generic `notify`
- heartbeat

Bot 처리:

- 모집 메시지 갱신.
- 라인업 이미지 발행.
- 종료 카드 발행.
- 경매 matchup/end card 발행.
- slash command 등록 이후 health/heartbeat 유지.

## Web Deep Links

Bot이 생성해야 하는 주요 웹 링크:

| 상황                            | URL                                             |
| ------------------------------- | ----------------------------------------------- |
| 일반 모집 마감 후 엔트리 편성   | `/g/{guildId}/recruitments/{id}/entry`          |
| 일반 시리즈 생성 후 픽/밴       | `/g/{guildId}/series/{id}/pick-ban`             |
| 일반 시리즈 완료 후 결과        | `/g/{guildId}/series/{id}/result`               |
| 경매 모집 마감 후 드래프트 시작 | `/g/{guildId}/auction/recruitments/{id}/draft`  |
| 경매 토너먼트 입찰 진행         | `/g/{guildId}/auction/tournaments/{id}/draft`   |
| 경매 브래킷 입력                | `/g/{guildId}/auction/tournaments/{id}/bracket` |
| 경매 토너먼트 완료 후 결과      | `/g/{guildId}/auction/tournaments/{id}/result`  |
| 사용자 프로필                   | `/users/{discordId}`                            |
| 리더보드                        | `/g/{guildId}/leaderboard`                      |
| 내 정보 / Riot 계정 관리        | `/me`                                           |
| audit log                       | `/g/{guildId}/admin?tab=logs`                   |

## Bot 우선순위

### P0

- `/내전모집`.
- 일반 모집 join/leave/role select.
- 일반 모집 start-entry link.
- series created/completed notify.

### P1

- 경매 모집 join/leave/start.
- 사용자 조회 commands.
- Riot 조회 commands.
- leaderboard/profile links.

### P2

- 관리자 destructive commands.
- `/로그` link.
- cleanup/stale commands.
- full end card/matchup card polish.
