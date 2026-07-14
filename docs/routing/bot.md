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
- API notification outbox claim 후 Discord 알림 발행.

Bot route의 비책임:

- 복잡한 도메인 상태 전이의 최종 판단.
- MMR 계산.
- audit log 직접 조작.
- 웹 화면 rendering.

인증 정책:

- Bot은 별도 앱 회원가입을 만들지 않는다.
- 사용자는 Discord interaction의 Discord user id로 식별한다.
- `/등록`은 앱 회원가입이 아니다. 현재 Discord guild에 `(guild_id, Discord user id)` 소속을 등록하고,
  같은 Discord user id에 Riot 계정을 연결하는 명령이다.
- 웹 콘솔 인증은 Discord OAuth2만 사용한다.

## 사용자 Slash Commands

### `/도움말`

목적:

- 사용자/내전/경매/운영자 도움말 표시.

Backend:

- 필요 없음 또는 최소 상태 확인.

### `/초기설정 운영자역할 모집채널 [길드이름] [엔트리채널] [결과알림채널] [운영로그채널]`

목적:

- bot이 설치된 Discord guild를 MonkeyBot 운영 공간으로 활성화한다.
- 선택한 Discord role ID를 관리 권한의 source로 저장한다.
- 모집 채널은 필수이며, 엔트리/결과/운영 로그 채널은 별도로 선택한다. 미선택 feedback channel은 `null`로
  저장하고 해당 Discord feedback을 발행하지 않는다.
- 길드 이름을 생략하면 현재 Discord guild 이름을 사용한다.
- 설정 완료 후 guild dashboard link를 제공한다.
- 최초 활성화에만 사용한다. 설정이 끝난 길드에서 새 interaction으로 다시 실행하면
  `guild_already_configured`를 반환하며 기존 설정을 덮어쓰지 않는다.
- 동일 interaction ID 재전송은 idempotent success replay로 처리한다.

Backend:

- guild setup API/service. Bot은 API 호출 전에 선택한 채널이 현재 guild의 게시 가능한 text/announcement
  channel인지 검증하고 `recruitmentChannelId`, nullable `entryChannelId`/`resultChannelId`/`logChannelId`를
  모두 보낸다. setup 직후 catalog sync가 수행되므로 backend는 이 command에서 catalog lookup을 하지 않는다.

권한:

- Discord guild 관리자 또는 bot 설치 권한자.

Web link:

- `/g/{guildId}`.

### `/등록 riot_id`

목적:

- Discord user id(snowflake) 기준 전역 사용자 record를 확인/upsert한다.
- interaction이 발생한 현재 guild에 `(guild_id, user_id)` 소속을 등록한다.
- 사용자가 입력한 `GameName#TagLine`을 검증하고, backend가 확인한 PUUID 기준으로 Riot 계정을
  전역 사용자에게 연결한다.
- 앱 회원가입이 아니라 길드 소속 등록과 Riot 프로필 연결이다.

Context:

- Discord guild 전용이다. DM에서는 실행할 수 없다.
- 실행자 본인만 등록하며 다른 Discord 사용자를 대신 등록하는 option은 두지 않는다.
- 처리 결과와 오류는 실행자에게만 보이는 ephemeral 응답으로 표시한다. 성공 시 canonical Riot ID와
  길드 소속/Riot 연결 상태를 embed로 요약하고 `${webOrigin}/me`로 이동하는 Link button을 제공한다.
  사용자는 해당 화면에서 선호 챔피언, 대표 길드, privacy와 Riot 계정을 이어서 관리한다.

처리 원칙:

- Discord interaction ID를 idempotency key로 사용한다.
- 같은 guild에서 반복 실행해도 `guild_members`나 같은 Riot 계정 연결을 중복 생성하지 않는다.
- 이미 다른 guild에 등록된 사용자라면 전역 사용자/Riot 계정은 재사용하고 현재 guild 소속만 추가한다.
- 기존 `guild_members.status=INACTIVE`는 자동으로 ACTIVE로 바꾸지 않는다. 운영자가 명시적으로
  재활성화하기 전까지 명령을 거절한다.
- 성공 응답은 현재 guild 소속 등록과 Riot 계정 연결이 함께 확정되었음을 뜻한다.

Backend:

- `POST /internal/guilds/{guildId}/registrations`.
- Bot은 `riot_id`를 GameName과 TagLine으로 분리하고 actor의 Discord user id, 전역 표시명, 현재 서버
  닉네임, avatar/role snapshot과 `gameName`, `tagLine`, `platform=KR`을 service principal 인증 및
  idempotency key와 함께 전달한다.

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

옵션:

- `형식`: 선택값 `BO1 | BO3 | BO5`, 생략 시 `BO3`.
- `하드피어리스`: boolean, 생략 시 `false`.

선택값은 모집 생성 request에 항상 명시적으로 전달한다. Bot은 series를 직접 만들지 않으며, Web의
엔트리 편성 전환이 모집의 규칙을 series immutable snapshot으로 복사한다.

처리 순서:

1. 공개 interaction을 즉시 defer한다.
2. interaction ID를 `Idempotency-Key`로 Axum create command에 전달한다.
3. response snapshot으로 모집 메시지를 게시한다.
4. 게시된 channel/message ID를 별도 idempotent bind command로 저장한다.

Web에서 생성된 모집은 interaction reply가 없다. Backend가 configured recruitment channel을 대상으로
`RECRUITMENT_MESSAGE_CREATE` notification을 transactionally enqueue하고, ready Bot worker가 같은
`renderRecruitmentMessage` renderer로 채널 message를 발행한다. send 성공 뒤 typed acknowledgement가
message reference를 원자적으로 bind한다.

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

현재 구현 범위는 `/경매모집`과 해당 Discord 모집 카드의 self-service 참가/취소/라인 선택, 그리고
`/경매조기종료`다. 이외 토너먼트 운영 command 항목은 설계 예약이며, route와 command registration이
생기기 전에는 실행 가능하다고 노출하지 않는다.

### `/경매모집 정원 [결승] [4강] [하드피어리스]`

목적:

- 10인 또는 20인 경매 모집 생성과 match rules 확정.
- Discord 경매 모집 메시지 발행.
- Web `/g/{guildId}/auction`의 경매 모집 dialog와 같은 aggregate를 생성.

Backend:

- `POST /api/guilds/{guildId}/auction-recruitments`의 Bot principal branch.
- Bot은 actor snapshot, `format: TEN_PERSON|TWENTY_PERSON`, 결승 BO1/BO3/BO5, 하드 피어리스와 20인일 때
  4강 BO1/BO3/BO5를 보낸다. 생략한 rule option은 BO3/일반으로 정규화한다. 10인에 4강 rule을 지정하면
  backend가 안전하게 거부한다. target count, active season, display number, setup draft/team slot은 backend
  create command가 결정한다.
- Web과 Bot은 별도 create service나 별도 모집 row를 만들지 않는다.
- Bot도 configured recruitment channel이 없으면 `recruitment_channel_not_configured` 응답을 그대로
  interaction에 표시한다. 성공 response의 channel/message ID는 notification worker typed acknowledgement가
  bind할 때까지 null일 수 있다.

Message components:

- 참석 button.
- 참석 취소 button.
- 참가 lane select. guild main lane을 기본값으로 사용하며 미설정 사용자는 lane 선택 후에만 참가를
  확정한다.

Web link:

- 생성 직후 `/g/{guildId}/auction/recruitments/{id}`.
- 운영자가 Web에서 모집을 마감한 뒤 `/g/{guildId}/auction/recruitments/{id}/draft`의 실제 마감 명단.

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

### `/경매조기종료 토너먼트 사유`

목적:

- 진행 중 경매 토너먼트를 history를 보존한 채 조기 종료한다.
- `토너먼트`는 경매 토너먼트 페이지 URL에 있는 canonical UUID다. `사유`는 공백이 아닌 1~240자다.

Backend:

- Bot은 먼저 BotBearer 전용
  `GET /api/guilds/{guildId}/auction-tournaments/{id}/cancel-preview`로 canonical tournament ID, 현재
  `revision`, status를 읽는다. 사용자가 revision을 입력하지 않는다.
- 이어서 `POST /api/guilds/{guildId}/auction-tournaments/{id}/cancel`을 service principal로 호출하며 actor
  snapshot, preview revision, 사유와 Discord interaction ID를 idempotency key로 보낸다.
- preflight와 cancel 사이에 tournament가 바뀌면 backend의 revision conflict를 그대로 표시하며, Bot이
  임의 재시도하거나 stale revision으로 취소하지 않는다.
- 성공 응답은 실행자에게만 보이며, source recruitment에 bound Discord 모집 메시지가 있으면 backend outbox가
  `AUCTION_RECRUITMENT_MESSAGE_DELETE`를 적재한다. CANCELLED tournament에는 결과 채널 카드를 새로
  발행하지 않는다.

권한:

- operator.

Audit:

- Backend가 actor, 사유, rollback된 세트 수를 기록한다.

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

- MVP에서는 제공하지 않는다. 롤백 기능을 도입할 때 별도 관리 진입점을 확정한다.

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

목적:

- 참석/취소.
- 선호 라인 선택.
- operator의 엔트리 편성 시작.

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

목적:

- 참석/취소.
- main lane이 없는 사용자의 lane 선택과 참가 확정.

경매 모집 마감과 취소는 Web 모집 화면의 권한 기반 lifecycle action으로만 제공한다. Discord
message component는 roster 참여 상태만 변경하며, 정원이 찬 뒤 운영자가 Web에서 마감하면
`/g/{guildId}/auction/recruitments/{id}/draft`의 실제 마감 명단으로 이동한다.

## API -> Bot Internal Routing

Bot은 ready 이후 polling worker로 internal notification outbox를 claim한다. 이 worker는 browser session을
사용하지 않고 Bot service principal과 고유 `instanceId`를 사용한다.

일반 모집 typed notification:

```text
RECRUITMENT_MESSAGE_CREATE
RECRUITMENT_MESSAGE_UPDATE
RECRUITMENT_MESSAGE_DELETE
payload = { schemaVersion: 1, recruitmentId, snapshot }
```

경매 모집 typed notification:

```text
AUCTION_RECRUITMENT_MESSAGE_CREATE
AUCTION_RECRUITMENT_MESSAGE_UPDATE
AUCTION_RECRUITMENT_MESSAGE_DELETE
payload = { schemaVersion: 2, auctionRecruitmentId, snapshot }
```

Web과 Bot command가 생성한 경매 모집 모두 configured recruitment channel에 위 CREATE row를
transactionally enqueue한다. Bot worker는 `renderAuctionRecruitmentMessage(snapshot)`으로 Discord 모집 메시지를 게시한 뒤
`POST /internal/notifications/auction-recruitment-message/ack`를 호출한다. 이 typed acknowledgement는
delivery 상태와 `auction_recruitments.channel_id/message_id` bind를 하나의 transaction으로 처리한다.
모집 마감은 bound message UPDATE를, OPEN 모집 취소는 DELETE를 적재한다. UPDATE/DELETE는 generic
`POST /internal/notifications/ack`로 완료 처리한다.

새 경매 모집 row는 `schemaVersion: 2`와 rules 포함 snapshot을 사용한다. 아직 claim되지 않은 이전 v1 row는
rules가 없으므로 Bot relay가 `BO3`, `hardFearless=false`로 정규화해 렌더링하며, typed acknowledgement는 v1/v2
모두 허용한다. backend는 기존 row를 재작성하거나 재발행하지 않는다.

확정 엔트리 notification:

```text
ENTRY_CONFIRMATION_CREATE
payload = { schemaVersion: 1, seriesId, snapshot }
```

`snapshot`은 series 생성 transaction에서 고정한 `bestOf`, `hardFearless`, 1팀/2팀의 lane별 roster,
`/g/{guildId}/series/{seriesId}/pick-ban` deep link를 담는다. configured entry channel이 있을 때만
생성되며, Bot은 snapshot을 SVG로 렌더링한 PNG 엔트리 카드와 `픽/밴 입력 열기` link button을 게시한 후
generic acknowledgement를 보낸다. Discord의 SVG preview 호환성에 의존하지 않으며, 재시도해도 같은
immutable snapshot으로 같은 roster를 전송한다.
delivery 실패는 series 생성 결과를 바꾸지 않고 notification outbox retry 정책으로 처리한다.

일반 시리즈 결과 notification:

```text
SERIES_RESULT_CREATE
payload = { schemaVersion: 2, seriesId, snapshot }
```

시리즈가 자연 종료(`SCORE_REACHED`)하거나 조기 종료(`EARLY_WIN|EARLY_DRAW`)로 `COMPLETED`가 될 때만
configured result channel에 적재한다. 목적지는 `result_channel_id`이며 `entry_channel_id` fallback은 없다.
`snapshot`은 immutable `id/displayNumber/title/bestOf/hardFearless`,
`TEAM_1|TEAM_2`의 lane-ordered roster, 최종 score, nullable 승리 팀, terminal reason, 확정 set마다
`winner/sidePreset`, 두 팀의 lane-ordered pick, lower-case `bans.blue`/`bans.red` 5칸, `patchVersion`,
`/g/{guildId}/series/{seriesId}/result` link를 담는다. Bot은 SVG 기반 PNG 결과 카드와 `결과 보기` link
button을 게시한 후 generic acknowledgement를 보낸다. MMR과 MMR delta는 결과 카드 payload에 넣지 않는다.

v2 결과 카드는 기존 결과 카드의 header와 최종 score를 유지한다. 참가자 roster는 팀별 한 번만 두고,
각 확정 set에는 팀별 5개 pick face와 BLUE/RED 5개 ban face만 배치한다. champion 이름, patch, player
이름, side 설명을 set마다 반복하지 않는다. Bot은 allowlist된 Data Dragon champion PNG만 process-local
cache로 가져와 SVG renderer에 주입한다. artwork fetch가 timeout/404/invalid image이면 해당 tile은 muted
empty slot으로 남기고 Discord 결과 알림과 acknowledgement는 계속 처리한다.

배포 전 이미 claim되지 않은 v1 compact outbox row는 그대로 존재할 수 있다. Bot은 `schemaVersion: 1`과 v2를
모두 parse하며, v1은 기존 compact renderer로, v2는 상세 renderer로 전송한다. backend는 새 row만 v2로
생성하며 기존 payload/deduplication key를 변환하거나 재발행하지 않는다.

result channel이 `NULL`이면 시리즈 완료는 정상 처리되고 결과 notification만 생략한다. delivery 실패도 완료
transaction을 rollback하지 않으며 기존 outbox retry 정책으로 처리한다. 이 첫 slice는 one-shot card만 발행한다.
이후 결과를 되돌리거나 재확정할 때 기존 Discord 카드를 edit/delete하는 `end_card_*` binding은 별도 lifecycle
확장으로 남긴다.

경매 토너먼트 완료 notification:

```text
AUCTION_TOURNAMENT_RESULT_CREATE
payload = {
  schemaVersion: 1,
  guildId, tournamentId, channelId,
  tournament: { displayNumber, title, format, semifinalBestOf, finalBestOf, hardFearless },
  champion: { teamId, teamName },
  matches,
  completedAt
}
```

결승이 확정되어 tournament가 `COMPLETED`가 될 때만 configured `result_channel_id`에 적재한다. 10인에서는
`semifinalBestOf`를 반드시 `null`로, 20인에서는 두 준결승과 결승의 완료 score를 모두 포함한다. 이 payload는
web 결과 aggregate를 재사용하지 않는 Bot 전용 v1 contract이며, 결과 channel이 없으면 completion은 그대로
성공하고 outbox만 생략한다.

처리 순서:

1. `POST /internal/notifications/claim`으로 최대 50개의 lease row를 claim한다.
2. payload version과 full snapshot을 검증하고 configured recruitment channel에 일반 모집은
   `renderRecruitmentMessage(snapshot)`, 경매 모집은 `renderAuctionRecruitmentMessage(snapshot)`을 보낸다.
   CREATE에는 `discordNonce`를 그대로 Discord nonce로 사용한다.
3. 일반 모집 CREATE send가 성공하면 `POST /internal/notifications/recruitment-message/ack`로 message ID를
   보낸다. 경매 모집 CREATE는
   `POST /internal/notifications/auction-recruitment-message/ack`를 사용한다. 각 typed acknowledgement는
   delivered 전이와 해당 recruitment message bind를 원자적으로 처리한다. 일반 모집 create와 경합한
   수동 추가/취소는 최신 UPDATE 또는 DELETE를 후속 적재한다.
4. 일반/경매 모집 UPDATE는 payload snapshot의 bound message를 각각의 renderer 결과로 edit한 뒤 generic
   `POST /internal/notifications/ack`를 호출한다. DELETE는 bound message를 삭제한 뒤 generic ack을
   호출한다. Discord가 이미 삭제된 message를 반환하면 DELETE delivery는 완료로 처리해 ack한다.
5. send/edit/delete 전에 실패하면 `POST /internal/notifications/nack`을 보낸다. 429/5xx/transport은 retry이고,
   ordinary 4xx 또는 delivery result가 불명확하면 terminal failure다.

UPDATE deduplication key는 recruitment revision을 포함하고 DELETE key는 recruitment마다 하나다. Browser
수동 추가와 soft-cancel은 bound message가 있을 때 각각 UPDATE와 DELETE를 적재한다. message bind 전의
mutation은 typed CREATE ack의 revision/status check가 보완한다.

## Web Deep Links

Bot이 생성해야 하는 주요 웹 링크:

| 상황                            | URL                                             |
| ------------------------------- | ----------------------------------------------- |
| 일반 모집 마감 후 엔트리 편성   | `/g/{guildId}/recruitments/{id}/entry`          |
| 일반 시리즈 생성 후 픽/밴       | `/g/{guildId}/series/{id}/pick-ban`             |
| 일반 시리즈 완료 후 결과        | `/g/{guildId}/series/{id}/result`               |
| 경매 모집 생성 직후             | `/g/{guildId}/auction/recruitments/{id}`        |
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
