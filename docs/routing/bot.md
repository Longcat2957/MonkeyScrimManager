# Bot Routing

작성일: 2026-07-15

## 문서 기준

이 문서는 현재 `apps/bot/src/commands/index.ts`에 등록된 Discord slash command와
`apps/bot/src/interactions/dispatcher.ts`가 처리하는 모집 카드 interaction만 설명한다.

- 현재 command registry와 handler가 이 문서보다 우선한다.
- API의 권한, 상태 전이, MMR, audit, notification outbox 판단은 Axum backend가 맡는다.
- Bot은 Discord option을 검증하고 actor snapshot, interaction ID, guild context를 API에 전달한다.
- 등록되지 않은 command나 Discord에서 직접 수행하지 않는 Web 작업을 사용 가능 기능처럼 안내하지 않는다.

전체 운영 흐름과 WebSocket 범위는
[Current Operating Contract](../12-current-operating-contract.md)를 따른다.

## 공통 규칙

- 모든 command는 Discord guild install, guild context에서만 실행된다. DM과 guild 밖 interaction은 거절한다.
- 실행자는 Discord user snowflake로 식별한다. 표시 이름이나 서버별 nickname은 식별자가 아니다.
- interaction ID를 API idempotency key로 전달한다. Discord 재전송이 별도 도메인 작업을 만들면 안 된다.
- backend가 길드 소속, operator 역할, lifecycle, revision을 최종 검증한다.
- 성공하지 못한 interaction은 사람이 읽을 수 있는 사유와 요청 참조 정보를 함께 ephemeral 응답으로 표시한다.
- Bot은 PostgreSQL을 직접 쓰지 않는다. 모든 mutation은 Bot service-principal API 호출을 통해 확정한다.
- 사용자에게 보이는 command 응답과 일반/경매 모집 카드는 Discord Components V2 container를 사용한다. `##` 제목, 짧은 상태 요약, 구분선, 세부 정보 또는 참가자 목록, 별도 action row 순서를 유지한다.
- accent color는 모집 상태와 성공/경고/오류만 구분하는 표현이다. Discord 메시지는 API aggregate의 source of truth가 아니며, 갱신 시 같은 V2 구조로 교체한다.

## 현재 등록 Command

| Command | 입력 | 현재 역할 |
| --- | --- | --- |
| `/초기설정` | `운영자역할`, `모집채널`, 선택 채널/이름 | Discord guild를 MonkeyBot 길드로 설정 |
| `/등록` | `riot_id` | Riot 계정 연결과 현재 guild 소속 등록 |
| `/내전모집` | 선택 `형식`, `하드피어리스` | 10인 일반 내전 모집 생성 |
| `/경매모집` | `정원`, 선택 `결승`, `4강`, `하드피어리스` | 10인/20인 경매 모집 생성 |
| `/경매인원추가` | `모집`, `멤버`, 선택 `라인` | 운영자의 경매 모집 참가자 추가 |
| `/경매인원삭제` | `모집`, `멤버` | 운영자의 경매 모집 참가자 제거 |
| `/경매목록` | 선택 `상태`, `시즌`, `limit` | 경매 토너먼트 목록과 Web 링크 조회 |
| `/경매강제취소` | `모집` | 모집 또는 연결된 경매 내전의 안전 취소 |
| `/경매조기종료` | `토너먼트`, `사유` | 진행 중 경매 토너먼트 조기 종료 |

## 길드 설정과 사용자 등록

### `/초기설정 운영자역할 모집채널 [길드이름] [엔트리채널] [결과알림채널] [운영로그채널]`

- Discord의 `Manage Guild` 권한이 기본 요구 조건이다.
- `운영자역할`과 `모집채널`은 필수다. `@everyone` 및 Discord가 관리하는 role은 운영자 역할로 선택할 수 없다.
- 모집 채널은 Bot이 메시지를 보낼 수 있는 현재 guild의 text/announcement channel이어야 한다.
- 엔트리, 결과 알림, 운영 로그 채널은 선택값이며, 미지정하면 backend 설정에 `null`로 전달한다.
- 길드 이름을 생략하면 Discord guild 이름을 사용한다.
- backend 설정 후 Discord channel/role catalog 동기화를 시도하고, 성공 응답에는 guild dashboard 링크를 반환한다.
- 이미 설정된 guild의 재실행 여부와 기존 설정 보존은 backend 응답을 그대로 표시한다. Bot이 설정을 임의로 덮어쓰지 않는다.

### `/등록 riot_id`

- 입력은 `GameName#TagLine` 형식이며, Bot은 형식을 먼저 검사한 뒤 KR platform Riot 조회를 backend에 요청한다.
- Discord user의 전역 사용자 record와 현재 guild의 membership을 upsert하고, 확인된 PUUID Riot 계정을 연결한다.
- 같은 사용자가 다른 guild에서 실행하면 전역 사용자/Riot 계정은 재사용하고 해당 guild 소속만 추가한다.
- 비활성 guild member, 이미 다른 Discord 사용자에게 연결된 Riot 계정, Riot 조회 실패는 backend 사유를 사용자에게 노출한다.
- 성공 응답은 canonical Riot ID와 길드 소속 상태를 보여 주고 `/me` 링크를 제공한다. 선호 챔피언, 대표 guild, privacy 설정은 Web의 `/me`에서 관리한다.

## 일반 내전 모집

### `/내전모집 [형식] [하드피어리스]`

- 형식은 BO1/BO3/BO5이며 생략 시 BO3이다.
- 하드 피어리스는 생략 시 `false`다.
- Bot과 Web은 같은 일반 모집 aggregate를 만든다. 모집 생성 권한과 현재 시즌 검증은 backend가 수행한다.
- 성공하면 Bot이 일반 모집 카드를 게시하고 message/channel reference를 API에 bind한다.

일반 모집 카드의 현재 interaction:

- `참가`: 실행자 본인을 모집에 추가한다.
- `참가 취소`: 실행자 본인을 모집에서 뺀다.
- `선호 라인`: TOP/JUNGLE/MID/BOTTOM/SUPPORT 복수 선택을 저장한다.
- `엔트리 편성 시작`: backend가 허용하는 상태와 권한일 때 모집을 엔트리 편성 단계로 전환하고 Web 작업 링크를 제공한다.

카드 갱신과 entry 확정/결과 알림은 backend notification outbox와 Bot notification relay가 처리한다. Bot 카드가 상태의 source of truth가 아니다.

## 경매 내전 모집과 운영

### `/경매모집 정원 [결승] [4강] [하드피어리스]`

- `정원`은 10 또는 20만 허용한다.
- 결승은 BO1/BO3/BO5이며 생략 시 BO3이다.
- 20인 모집만 4강 BO1/BO3/BO5를 받을 수 있다. 10인에서 `4강` 입력은 즉시 거절한다.
- 하드 피어리스는 생략 시 `false`다.
- Bot과 Web은 동일한 auction recruitment aggregate를 생성한다. Bot은 성공 뒤 모집 Web 링크를 ephemeral 응답으로 제공한다.

경매 모집 카드의 현재 interaction:

- `참가`: 실행자를 현재 모집에 추가한다.
- `참가 취소`: 실행자를 현재 모집에서 뺀다.
- `참가 라인`: 정확히 한 개의 라인을 선택해 해당 선택으로 참가한다.

모집 card의 생성·갱신·삭제는 API notification outbox가 Discord relay에 전달한다. 입찰, 브래킷, lineup, match의 상세 조작은 Web workflow에서 수행하며 현재 별도 Discord command가 없다.

### `/경매인원추가 모집 멤버 [라인]`

- 모집 UUID와 Discord 멤버를 입력한다.
- 대상의 저장된 guild main lane 또는 입력 `라인`을 이용해 OPEN 경매 모집에 추가한다.
- 실행자가 대상과 다르면 backend가 configured operator인지 검증한다.

### `/경매인원삭제 모집 멤버`

- 모집 UUID와 Discord 멤버를 입력한다.
- backend가 operator 권한, 모집 상태, 실제 참가 여부를 확인한 뒤 참가자를 제거한다.

### `/경매목록 [상태] [시즌] [limit]`

- `상태`는 `진행 중` 또는 `완료`이며 기본값은 진행 중이다.
- `시즌`은 선택 UUID filter다.
- `limit`은 Discord 응답 길이를 위해 1~10 범위다.
- 결과에는 각 토너먼트의 Web 링크와 `/g/{guildId}/auction` 전체 보기 링크를 포함한다.

### `/경매강제취소 모집`

- 모집 UUID를 입력한다.
- OPEN/CLOSED 모집은 모집을 취소하고, 연결된 진행 중 tournament는 backend 안전 조건을 만족할 때 함께 취소한다.
- 확정 game이 있으면 backend는 MMR history tail 검증을 통과한 경우에만 관련 세트와 MMR 반영을 되돌린다.
- 원장 row를 Bot이 직접 삭제하지 않는다.

### `/경매조기종료 토너먼트 사유`

- 토너먼트 UUID와 1~240자의 사유를 입력한다.
- Bot은 먼저 최신 revision을 읽고, 그 revision과 interaction ID를 가진 조기 종료 command를 backend에 전달한다.
- 경쟁 변경으로 revision conflict가 나면 stale retry를 하지 않고 backend 오류를 사용자에게 표시한다.

## 현재 미등록 범위

다음은 현재 Discord command registry에 없으므로 Bot에서 제공하거나 사용자에게 안내하지 않는다.

- `/도움말`, `/내정보`, `/내정보갱신`, `/내전기록`, `/랭킹`, `/전적`
- 일반 내전의 `/내전인원추가`, `/내전인원삭제`, `/내전목록`, `/내전강제취소`, `/내전조기종료`
- `/일괄등록`, `/mmr수정`, `/새시즌`, `/오래된내전정리`, `/로그`
- 일반/경매 내전의 entry 배치, 픽밴 입력, 입찰 확정, 브래킷 편성, lineup lock, 결과 입력을 직접 수행하는 Discord command

이 작업들은 현재 Web route 또는 backend 내부 notification 흐름에 속한다. 향후 command를 추가할 때는 command registration, Bot API client, backend service-principal branch, idempotency/error contract, 사용자 문서를 함께 추가한다.
