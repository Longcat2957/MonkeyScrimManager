# Bot Routing

작성일: 2026-07-10

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

### `/초기설정 guild_name admin_role_name`

목적:

- bot이 설치된 Discord guild를 MonkeyScrimManager 운영 공간으로 활성화한다.
- guild 표시 이름과 관리 권한 role 이름만 설정한다.
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

- user profile/MMR API.

Web link:

- `/g/{guildId}/users/{discordId}`

### `/내정보갱신`

목적:

- Riot 소환사 아이콘 재동기화.

Backend:

- riot account refresh API.

### `/내전기록 [user]`

목적:

- 현재 guild 기준 라인별 통계, 최근 내전, MMR 변동 조회.

Backend:

- user records/history API.

Web link:

- `/g/{guildId}/users/{discordId}`
- `/g/{guildId}/records?user={discordId}`

### `/랭킹 [라인]`

목적:

- 라인별 시즌 MMR 상위권 조회.

Backend:

- leaderboard API.

Web link:

- `/g/{guildId}/leaderboard`

### `/전적 [riot_id|user]`

목적:

- 일반 전적 검색.
- 등록된 Discord user 또는 Riot ID를 기준으로 내부 내전 전적과 Riot 외부 요약을 조회.
- 내부 내전 전적은 현재 guild 안의 기록만 보여준다.

Backend:

- records search API.
- Riot lookup API/core service.

Web link:

- `/g/{guildId}/records?q={query}`

### `/지금게임 [riot_id|user]`

목적:

- Riot Spectator 현재 게임 조회.

Backend:

- spectator API/core service.

### `/전적검토 [riot_id|user]`

목적:

- 최근 게임 기반 위험도/표본/챔피언 풀/시간대 리포트.

Backend:

- screening API.

## 일반 내전 운영 Commands

### `/내전모집 정원`

목적:

- 1v1~5v5 일반 모집 생성.
- Discord 모집 메시지 발행.

Backend:

- recruitment create API 또는 bot용 service endpoint.

Message components:

- 참석 button.
- 참석 취소 button.
- 선호 라인 multi-select.
- 정원 도달 시 operator action.

Web link:

- 정원 도달/마감 후 `/recruitments/{id}/entry`.

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

- `POST /api/recruitments/:id/reopen` 또는 bot용 service.

권한:

- operator.

### `/내전목록 [상태] [시즌] [limit]`

목적:

- 시리즈 목록 조회.

Backend:

- `GET /api/series`
- `GET /api/series/completed`

Web link:

- `/series`
- `/series/completed`

### `/내전강제삭제 series_id [rollback_mmr]`

목적:

- 시리즈 강제 삭제 및 선택적 MMR 롤백.

Backend:

- series destructive admin API/service.

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
- 정원 도달 시 operator action.

Web link:

- `/auction/recruitments/{id}/draft`.

### `/경매인원추가 모집 멤버`

목적:

- 경매 모집 강제 참가.

Backend:

- `POST /api/auction-recruitments/:id/members`.

권한:

- operator.

### `/경매인원삭제 모집 멤버`

목적:

- 경매 모집 강제 제거.

Backend:

- `DELETE /api/auction-recruitments/:id/members/:uid`.

권한:

- operator.

### `/경매목록 [상태] [시즌] [limit]`

목적:

- 경매 토너먼트 목록 조회.

Backend:

- auction tournament list API.

Web link:

- `/auction`

### `/경매강제삭제 모집`

목적:

- 경매 모집과 토너먼트 응급 삭제.

Backend:

- auction destructive admin API/service.

권한:

- operator/admin.

Audit:

- Backend에서 기록.

### `/경매조기종료 토너먼트`

목적:

- 진행 중 경매 토너먼트 취소.

Backend:

- `POST /api/auction-tournaments/:id/cancel` 또는 bot용 service.

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

### `/시즌결과리셋 season_id`

목적:

- 시즌 RANKED/AUCTION 데이터 정리.

Backend:

- season reset API/service.

권한:

- admin.

Audit:

- Backend에서 기록.

### `/오래된내전정리 [days]`

목적:

- 방치 모집/시리즈/경매 일괄 정리.

Backend:

- cleanup stale API/service.

권한:

- admin/operator.

### `/로그`

목적:

- audit log 웹뷰 링크 발급.

Web link:

- `/g/{guildId}/admin/logs`.

### `/되돌리기 [target]`

목적:

- 현재 guild에서 최근 revert checkpoint를 되돌린다.
- target이 없으면 호출한 채널/최근 작업 기준으로 되돌린다.
- target이 있으면 series, auction tournament, auction match 등 범위를 좁힌다.
- 범용 undo/redo가 아니라 엔트리 편성, 마지막 게임, 경매 입찰 단계, 라인업 확정 취소 같은 고정 단계 복구다.

Backend:

- `POST /api/guilds/:guildId/reverts/apply-last`.

권한:

- operator/admin.

Audit:

- Backend에서 기록.

Web link:

- `/g/{guildId}/admin/logs`.

### `/유저강제삭제 discord_id`

목적:

- 사용자 soft-delete와 관련 데이터 요약/정리.

Backend:

- force delete user API/service.

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

- `recruit:start-entry` 처리 후 `/recruitments/{id}/entry`.

### 경매 모집 message components

Handlers:

- `auction-recruit:join:{recruitmentId}`
- `auction-recruit:leave:{recruitmentId}`
- `auction-recruit:start:{recruitmentId}`
- `auction-recruit:cancel:{recruitmentId}`
- `auction-recruit:cancel-confirm:{recruitmentId}`

목적:

- 참석/취소.
- operator의 경매 시작.
- 모집 취소 확인.

Web link:

- `auction-recruit:start` 처리 후 `/auction/recruitments/{id}/draft`.

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

| 상황 | URL |
| --- | --- |
| 일반 모집 마감 후 엔트리 편성 | `/recruitments/{id}/entry` |
| 일반 시리즈 생성 후 픽/밴 | `/series/{id}/pick-ban` |
| 일반 시리즈 완료 후 결과 | `/series/{id}/result` |
| 경매 모집 마감 후 드래프트 시작 | `/auction/recruitments/{id}/draft` |
| 경매 토너먼트 입찰 진행 | `/auction/tournaments/{id}/draft` |
| 경매 브래킷 입력 | `/auction/tournaments/{id}/bracket` |
| 경매 토너먼트 완료 후 결과 | `/auction/tournaments/{id}/result` |
| 사용자 프로필 | `/users/{discordId}` |
| 리더보드 | `/leaderboard` |
| 내 Riot 계정 관리 | `/me/riot-accounts` |
| audit log | `/admin/logs` |

## Bot 우선순위

### P0

- `/내전모집`.
- 일반 모집 join/leave/role select.
- 일반 모집 start-entry link.
- series created/completed notify.
- `/로그` link.

### P1

- 경매 모집 join/leave/start.
- 사용자 조회 commands.
- Riot 조회 commands.
- leaderboard/profile links.

### P2

- 관리자 destructive commands.
- cleanup/stale commands.
- full end card/matchup card polish.
