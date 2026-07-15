# Realtime Collaboration

작성일: 2026-07-15

## 목적

이 문서는 MonkeyBot의 동시 편집과 WebSocket 동기화에 대한 단일 기준이다.

여러 운영자가 동시에 다룰 수 있는 화면 중 WebSocket을 실제로 연결하는 범위는 일반 엔트리 편성, 일반
픽밴, 경매 모집 roster, `BIDDING` 상태의 경매 입찰이다. 경매 setup, bracket, lineup, match 결과 입력은
동시 접근해도 HTTP snapshot과 revision conflict refetch만 사용한다.

경매 모집 roster와 `BIDDING` 상태의 경매 입찰은 active operator의 invalidate 구독을 사용한다. 입찰 command는
HTTP snapshot, expected revision, `409` refetch로 충돌을 처리하고, WebSocket은 최신 snapshot 재조회 신호만
전달한다.

구현과 문서가 충돌하면 실제 route/API/Bot 동작을 먼저 따르고 이 문서를 수정한다. 현재 운영 범위는
[Current Operating Contract](12-current-operating-contract.md), DB 구조는 `05-db-schema.md`, HTTP route와 topic은
`routing/backend.md`를 따른다.

## 결정 사항

```txt
PostgreSQL             = 영속 상태와 revision의 source of truth
HTTP API               = 모든 상태 변경과 권한 검증
WebSocket              = invalidate 전달
Valkey                 = API instance 간 Pub/Sub과 rate limit
Transactional outbox   = commit된 invalidate의 전달 backlog
Frontend local state   = 아직 저장되지 않은 사용자 입력
```

필수 원칙:

- WebSocket message만으로 DB 원장 상태를 확정하지 않는다.
- 유실되면 안 되는 draft와 경매 상태를 Valkey에만 저장하지 않는다.
- 같은 데이터를 DB와 Valkey에 이중 write하지 않는다.
- 중요한 운영 상태에 last-write-wins를 사용하지 않는다.
- 여러 사용자가 수정하는 aggregate를 전체 JSON으로 반복 덮어쓰지 않는다.

## 하지 않는 것

MVP에서는 다음을 구현하지 않는다.

- CRDT 또는 Operational Transformation.
- WebSocket 기반 전체 state patch 전송.
- 범용 실시간 문서 편집 엔진.
- 서로 다른 필드의 자동 merge를 위한 복잡한 diff engine.
- Valkey/distributed lock 구현.
- offline mutation queue.

MVP의 충돌 정책은 `revision 검증 -> 409 Conflict -> 최신 snapshot 조회 -> 사용자 재적용`이다.

## Aggregate와 Revision

동시 편집 단위마다 하나의 aggregate revision을 가진다. 일반 내전 모집과 엔트리 draft는 같은
`recruitmentId`를 사용해도 별도 aggregate다.

```txt
recruitments.revision                모집 상태, roster, Discord message lifecycle
recruitment_entry_drafts.revision    참가자 배치와 entry workspace
auction_setup_drafts.revision
series.revision
auction_tournament.revision
auction_match.revision
```

규칙:

- revision은 0 이상의 정수다.
- aggregate를 변경하는 transaction마다 정확히 1 증가한다.
- 실패하거나 rollback된 transaction은 revision을 증가시키지 않는다.
- audit log, updated_at, updated_by는 같은 transaction에서 기록한다.
- 같은 aggregate의 모든 GET 응답과 mutation 응답에 revision을 포함한다.
- `guild:{guildId}:recruitment:{recruitmentId}` invalidate의 revision은 반드시
  `recruitments.revision`이다.
- `guild:{guildId}:recruitment-entry-draft:{recruitmentId}` invalidate의 revision은 반드시
  `recruitment_entry_drafts.revision`이다.
- 한 command가 모집과 entry draft를 함께 바꾸면 각 aggregate의 revision을 독립적으로 올린다. outbox는
  해당 화면을 invalidate해야 하는 aggregate에 대해 하나 또는 두 topic을 선택할 수 있지만, 한 topic에
  다른 aggregate revision을 넣지 않는다. 엔트리 route는 어느 topic event든 받은 뒤 composite snapshot을
  다시 조회한다.

조회 응답 예시:

```json
{
	"data": {
		"id": "0190f480-0000-7000-8000-000000000002",
		"revision": 42,
		"updatedAt": "2026-07-11T11:20:00Z",
		"updatedBy": "328516947210438656",
		"draft": {}
	}
}
```

Mutation 요청 예시:

```json
{
	"expectedRevision": 42,
	"operation": {
		"type": "SET_PICK",
		"setNumber": 2,
		"userId": "328516947210438656",
		"championId": "Ahri"
	}
}
```

성공 응답 예시:

```json
{
	"data": {
		"revision": 43,
		"updatedAt": "2026-07-11T11:20:12Z",
		"updatedBy": "328516947210438656"
	}
}
```

충돌 응답 예시:

```json
{
	"error": {
		"code": "revision_conflict",
		"details": {
			"expectedRevision": 42,
			"currentRevision": 43,
			"updatedAt": "2026-07-11T11:20:08Z",
			"updatedBy": "954301284660912138"
		}
	}
}
```

## Mutation 단위

동시 편집 화면은 사용자가 수행한 최소 의미 단위로 mutation한다.

| 화면        | 허용 mutation 예시                              | 금지 방식                      |
| ----------- | ----------------------------------------------- | ------------------------------ |
| 엔트리 편성 | 참가자 1명을 특정 팀/라인으로 이동, 슬롯 비우기 | 전체 배정 JSON PUT 반복        |
| 픽밴        | 특정 game/team/slot의 pick 또는 ban 설정/해제   | 전체 시리즈 draft 덮어쓰기     |
| 경매 준비   | 팀장 1명 지정, 팀 포인트 변경                   | 전체 tournament 객체 저장      |
| 경매 입찰   | draw, 유찰, 낙찰, 복구, 수동 배치               | 브라우저가 낙찰 결과 직접 계산 |
| 경매 라인업 | 선수 1명의 role 배정, lineup lock               | 전체 bracket 덮어쓰기          |
| 결과 입력   | 단일 game 결과 생성, 마지막 game 취소           | 클라이언트가 MMR 직접 계산     |

추가 규칙:

- 서로 독립적인 셀이라도 server가 자동 merge하지 않는다. 최신 revision을 기준으로 다시 요청한다.
- 낙찰, 결과 입력, lineup lock 같은 command는 autosave하지 않는다.
- 클라이언트는 command 실행 중 같은 버튼을 다시 실행하지 못하게 한다.
- 중복 불가 작업은 DB의 현재 상태 조건과 unique constraint로 한 번만 commit한다.

## Mutation 처리 순서

Backend는 다음 순서를 지킨다.

```txt
1. session 확인
2. guild membership / operator 권한 확인
3. request schema 검증
4. DB transaction 시작
5. aggregate와 현재 revision 조회
6. expectedRevision 비교
7. domain invariant 검증
8. 상태 변경, revision + 1, audit/checkpoint/outbox 기록
9. PostgreSQL transaction commit
10. 새 revision을 HTTP response로 반환
11. outbox relay가 Valkey Pub/Sub에 invalidate publish
```

주의:

- 6번이 실패하면 `409 Conflict`를 반환하고 아무 상태도 변경하지 않는다.
- Pub/Sub publish 실패로 domain commit을 rollback하지 않는다.
- publish 실패는 outbox backlog로 남기고 retry하며 error log와 metric을 기록한다.
- relay는 짧은 DB transaction에서 row를 lease한 뒤 commit하고, Valkey publish는 transaction
  밖에서 수행한다. 외부 network I/O 동안 `FOR UPDATE` lock을 유지하지 않는다.
- 각 claim은 새 lease token을 발급하고 ack/nack은 owner와 token이 모두 일치할 때만 반영한다.
  만료된 이전 claim의 늦은 응답이 재claim된 row를 덮어쓰지 못해야 한다.
- HTTP 성공 응답 자체가 현재 client를 동기화할 수 있어야 한다.
- 다른 client는 reconnect, window focus, 다음 invalidate에서 누락을 복구한다.

## WebSocket 계약

### 연결과 인증

- WebSocket은 기존 Discord OAuth session으로 인증한다.
- subscribe 요청마다 guild membership과 resource 조회 권한을 확인한다.
- client가 임의 guild 또는 다른 사용자의 topic을 구독하게 두지 않는다.
- session이 만료되거나 권한이 제거되면 해당 subscription을 종료한다.
- browser는 같은 origin의 `wss://<origin>/ws`(local에서는 `ws://`)만 연결한다. handshake에는 정확히 하나의
  `Origin` header가 `MONKEYBOT_PUBLIC_ORIGIN`과 일치해야 한다.
- handshake 전 session 검사를 수행한다. 인증되지 않은 요청은 websocket close가 아니라 일반 `401` JSON 오류로
  끝난다.
- frame/message 크기는 8 KiB, socket당 topic은 16개로 제한한다. server는 25초 ping, 75초 idle timeout을
  사용하며 느린 client는 close할 수 있다.

### Topic 형식

```txt
guild:{guildId}:recruitment:{recruitmentId}
guild:{guildId}:recruitment-entry-draft:{recruitmentId}
guild:{guildId}:series:{seriesId}
guild:{guildId}:auction-recruitment:{recruitmentId}
guild:{guildId}:auction-tournament:{tournamentId}
guild:{guildId}:auction-match:{matchId}
```

엔트리 편성 route는 제한적 realtime 단계가 활성화되면 두 topic을 모두 구독한다. 모집 topic은
roster/status/capability 변경을, entry-draft topic은 assignment/draft metadata 변경을 알린다.

`auction-tournament` topic은 `BIDDING` 상태의 active operator만 구독한다. 경매 입찰 command는 HTTP
transaction으로 확정하고, WebSocket은 `invalidate`와 GET snapshot 재조회만 담당한다. 일반 길드 멤버,
`BRACKET_SETUP` 이후의 tournament, `auction-match`는 이 topic을 구독하지 않는다.

현재 WebSocket을 사용하지 않는 HTTP-only route는 이 topic을 구독하지 않고 다음 snapshot 조회로 복구한다.

현재 subscribe를 허용하는 범위는 일반 내전 엔트리의 아래 두 topic, 진행 중 일반 series, OPEN/CLOSED
경매 모집의 operator roster topic, `BIDDING` 경매 토너먼트의 operator 입찰 topic이다.

```txt
guild:{guildId}:recruitment:{recruitmentId}
guild:{guildId}:recruitment-entry-draft:{recruitmentId}
guild:{guildId}:series:{seriesId}
guild:{guildId}:auction-recruitment:{recruitmentId}
guild:{guildId}:auction-tournament:{tournamentId}
```

entry topic은 해당 guild의 active member가 구독할 수 있다. series topic은 `IN_PROGRESS` series의 current
active operator만 구독할 수 있다. auction recruitment topic은 OPEN/CLOSED 모집의 current active operator만
구독할 수 있다. auction tournament topic은 `BIDDING` 상태의 current active operator만 구독할 수 있다. 일반
member와 result ledger는 socket을 열지 않고 HTTP snapshot을 다시 읽는다. auction match를 포함한 나머지 topic
형식은 outbox/namespace의 예약 계약이며 해당 화면의 HTTP snapshot, 권한 검사, frontend 재조회가 함께 준비되기
전에는 `/ws` subscribe가 `unsupported_topic`을 반환한다.

### Client frame

```json
{ "type": "subscribe", "topics": ["guild:123:recruitment:0190f4a2-8b3c-7d5e-8f01-23456789abcd"] }
```

```json
{ "type": "unsubscribe", "topics": ["guild:123:recruitment:0190f4a2-8b3c-7d5e-8f01-23456789abcd"] }
```

- unknown field, empty/duplicate/non-canonical topic은 거부한다.
- subscribe 전체를 먼저 parse하고 각 topic의 guild membership, 현재 resource visibility, 필요한 경우
  operator role을 확인한다.
- client는 필요한 두 topic을 한 `subscribe` frame으로 보내고, 두 topic 모두 들어 있는 `subscribed` ack를
  받은 뒤에만 `connected`를 표시한다.

### Server control frame

```json
{ "type": "subscribed", "topics": ["guild:123:recruitment:0190f4a2-8b3c-7d5e-8f01-23456789abcd"] }
```

```json
{ "type": "resync_required", "topics": ["guild:123:recruitment:0190f4a2-8b3c-7d5e-8f01-23456789abcd"] }
```

```json
{ "type": "error", "code": "subscription_forbidden", "message": "해당 길드의 실시간 정보를 볼 권한이 없습니다." }
```

`resync_required`는 bounded local hub에서 lag가 발생했을 때 보내며, client는 revision 비교 없이 route의
authoritative GET snapshot을 다시 읽는다. `error`는 민감한 내부 원인을 포함하지 않는다.

MVP에서는 dashboard, 목록, records, leaderboard, profile, admin, result 원장에 WebSocket을 사용하지
않는다. 경매 result의 operator 전용 checkpoint revert도 단일 HTTP command 후 snapshot을 갱신하며
협업 topic을 추가하지 않는다.

### Invalidate event

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

규칙:

- event에 전체 aggregate 또는 민감한 payload를 넣지 않는다.
- 단일 resource topic은 `resourceId`와 `revision`을 포함한다.
- recruitment topic event는 `snapshot.recruitment.revision`과만 비교하고, entry-draft topic event는
  `snapshot.draft.revision`과만 비교한다. 두 revision은 같은 숫자여도 상호 대체할 수 없다.
- client는 현재 revision 이하의 중복 또는 역순 event를 무시한다.
- 현재 revision보다 큰 event를 받으면 HTTP GET으로 snapshot을 다시 조회한다.
- WebSocket event의 도착 순서와 HTTP response의 도착 순서는 보장된다고 가정하지 않는다.
- outbox 전달은 at-least-once이므로 client는 eventId와 revision으로 중복을 허용하고 제거한다.

## Relay 실행 경계

각 API process는 process-wide outbox relay 하나를 실행한다. relay는 guild를 순회하지 않고
`FOR UPDATE SKIP LOCKED`로 publish 가능한 row를 claim한 뒤 claim transaction을 먼저 commit한다. 이후에만
Valkey `PUBLISH`를 수행하고, 성공하면 같은 lease owner/token으로 ack한다. publish 실패나 invalid event는
짧은 retry delay로 nack한다. 외부 I/O 중 PostgreSQL row lock을 유지하지 않는다.

Valkey가 설정된 development/production instance는
`monkeybot:{env}:pubsub:{topic}` pattern을 구독해 local hub로 fan-out한다. publishing instance도 local hub에
즉시 fan-out할 수 있으므로 subscriber 재연결 순간의 손실을 줄인다. 이 경로는 의도적으로 at-least-once이며
중복 invalidate가 가능하다.

Local/test에서 `MONKEYBOT_VALKEY_URL`이 비어 있으면 local hub relay를 사용한다. 이는 단일 API process
개발 편의용일 뿐 multi-instance delivery를 보장하지 않는다. shared development traffic을 함께 검증할 때는
반드시 private Valkey URL을 설정한다.

## Frontend 상태 머신

현재 frontend 계약처럼 연결 상태와 저장 상태를 분리한다.

```txt
connectionState = connected | reconnecting | offline
saveState       = saved | saving | dirty | conflict
```

`loading`은 route/snapshot fetch 상태이며 위 두 enum에 넣지 않는다. `RealtimeEditStateBar`는 두 축을 조합해 간결하게 표시한다.

```txt
connected + saved -> 연결 아이콘 + 저장됨
dirty        -> 연결 아이콘 + 저장 대기
saving      -> 연결 아이콘 + 저장 중
reconnecting -> 재연결 중
offline      -> 연결 끊김 + 재연결 액션
conflict     -> 변경 충돌 + 최신 상태 반영/로컬 입력 유지
```

## 현재 저장 방식

엔트리 이동과 일반 픽/밴 cell 변경은 사용자의 action마다 작은 HTTP mutation으로 저장한다. 현재 운영
route는 400~800ms debounce나 background autosave queue를 계약으로 두지 않는다.

- aggregate별 in-flight mutation은 최대 1개로 유지한다.
- 성공 응답의 authoritative snapshot/revision으로 화면을 교체한다.
- `409 Conflict`가 오면 자동 재시도하지 않고 최신 snapshot을 읽는다.
- 경매 입찰, lineup lock, game submit 같은 위험 command는 명시 action으로만 실행한다.
- 전체 JSON draft replace와 client-side merge는 사용하지 않는다.

## Invalidate와 로컬 입력

Invalidate 수신 시 local state 처리:

```txt
saveState = saved
  -> 최신 snapshot 조회
  -> revision과 화면 상태 교체

saveState = dirty 또는 saving
  -> 최신 snapshot을 별도로 조회
  -> 편집 중 값을 즉시 덮어쓰지 않음
  -> 같은 대상이 변경되었으면 conflict
  -> 영향 없는 대상이면 최신 revision 기준으로 local operation 재검증
```

MVP에서는 영향 없는 필드의 자동 merge도 최소화한다. 안전 여부를 domain operation 단위로 명확히 판정할 수 없으면 conflict로 처리한다.

충돌 UI에는 다음 정보만 표시한다.

- 충돌한 작업 대상.
- 마지막 변경자.
- 마지막 변경 시각.
- 최신 상태 반영.
- local operation을 최신 revision에 다시 적용.

서버 값과 local 값의 거대한 JSON diff를 사용자에게 노출하지 않는다.

## 재연결과 복구

WebSocket이 끊기면:

```txt
1. UI를 reconnecting으로 변경
2. local dirty state 유지
3. exponential backoff + jitter로 재연결
4. 인증 재확인
5. 현재 route topic 재구독
6. 현재 aggregate 전체 GET
7. revision 비교
8. synced 또는 conflict로 전환
```

추가 재검증 시점:

- `visibilitychange`로 page가 다시 활성화될 때.
- browser가 online 상태로 돌아올 때.
- 위험 command 실행 직전.
- HTTP mutation이 timeout되었을 때.

HTTP timeout은 실패 확정이 아니다. server에서 commit되었을 수 있으므로 같은 command를 즉시 반복하지 않고 snapshot을 먼저 조회한다.

## Valkey 사용 규칙

Valkey에 저장 가능한 것:

```txt
monkeybot:{env}:pubsub:{topic}                 # instance 간 Pub/Sub
monkeybot:{env}:ratelimit:{principal}:{route}  # 보조 rate limit
```

Valkey에만 저장하면 안 되는 것:

- 엔트리 배정 draft.
- 픽밴 draft.
- 경매 현재 단계와 현재 매물.
- 실제 bid와 낙찰 결과.
- 팀 포인트와 선수 배치.
- lineup lock과 game result.
- MMR 변경.

분산 lock은 MVP Valkey 범위에 포함하지 않는다. 동시 실행 정합성은 PostgreSQL row lock, revision,
state condition, unique constraint가 보장한다.

## 경매 내전 추가 규칙

경매는 일반 draft보다 경쟁 조건의 영향이 크므로 다음을 추가로 지킨다.

### Draw

- draw 대상과 tournament stage를 transaction 안에서 검증한다.
- 이미 active lot이 있으면 새 draw를 거부한다.
- draw 결과와 revision을 DB에 commit한 뒤 invalidate한다.

### 지원 command

현재 frontend command union만 지원한다.

```text
DRAW_NEXT
MARK_UNSOLD
AWARD_BID
ASSIGN_CURRENT
ASSIGN_UNSOLD
RESTORE_UNSOLD
REVERT_LAST
```

bid intent와 실시간 호가 목록은 현재 frontend workflow에 없으므로 MVP API, DB, Valkey에
추가하지 않는다.

`RESTORE_UNSOLD`는 active target이 없을 때 선택한 유찰 선수를 즉시 current target으로
복원한다. 일반 queue 복귀나 재추첨 command로 해석하지 않는다.
`REVERT_LAST`가 낙찰, current 수동 배치, 유찰을 되돌려 선수를 current target으로 복원할 때는
이전 lot을 `REVERTED`로 보존하고 새 ACTIVE lot을 만든다.

### Finalize bid

하나의 DB transaction에서 다음을 처리한다.

```txt
active lot 확인
expectedRevision 확인
winning team / bid 검증
남은 point 검증 및 차감
선수 team 배치
AWARD_BID action / point ledger 기록
revert checkpoint 기록
tournament revision 증가
```

동시 finalize 요청 중 하나만 성공해야 한다.

### Lineup과 결과

- lineup은 role별 mutation으로 저장한다.
- lineup lock은 5개 role 완성, 선수 중복, team membership을 transaction에서 검증한다.
- locked lineup의 revision을 기준으로만 game result를 입력한다.
- game 생성, MMR 변경, series/match score 변경은 하나의 transaction으로 처리한다.

현재 API는 browser editor presence를 발행하거나 화면에 표시하지 않는다. `RealtimeEditStateBar`는
연결/저장 상태만 보여 주며, 접속자 수나 이름을 권한 판단과 편집 lock에 사용하지 않는다.

## 관측 가능성

최소 로그:

- WebSocket connect/disconnect/reconnect.
- subscribe 거부와 권한 실패.
- revision conflict.
- mutation latency와 실패 원인.
- outbox publish 실패와 backlog.
- 경매 finalize 경쟁 요청과 거부 사유.

권장 metric:

```txt
ws_connections
ws_reconnects_total
ws_publish_failures_total
outbox_backlog
revision_conflicts_total{resource_type}
mutation_duration_ms{operation}
mutation_rejected_total{reason}
```

로그에 전체 draft, Discord token, session cookie, Riot API key를 기록하지 않는다.

## 필수 테스트

### Contract test

- 모든 동시 편집 GET 응답에 revision이 있다.
- 모든 mutation이 expectedRevision을 검증한다.
- conflict 응답 형식이 동일하다.
- event schema와 guild topic 형식이 고정되어 있다.

### API integration test

- 같은 revision으로 동시에 요청하면 하나만 성공한다.
- 실패 transaction은 revision을 증가시키지 않는다.
- 낙찰 시 point 차감과 선수 배치가 함께 commit된다.
- lineup lock과 결과 입력 invariant가 transaction에서 검증된다.
- 권한 없는 guild topic 구독이 거부된다.

### Two-client browser test

```txt
Client A와 B가 같은 화면 진입
A가 수정 후 저장
B가 invalidate 수신 후 재조회
B의 dirty 입력과 A의 변경이 겹치면 conflict
B가 최신 상태 반영 후 정상 저장
```

### Failure test

- WebSocket event 중복.
- WebSocket event 역순.
- invalidate 누락.
- HTTP response보다 invalidate가 먼저 도착.
- WebSocket 단절 중 다른 client가 수정.
- HTTP timeout 뒤 실제 commit 완료.
- Valkey restart 또는 flush.
- API instance 2개에서 같은 경매 finalize 요청.

완료 기준은 event가 항상 전달되는 것이 아니다. event가 누락되거나 순서가 바뀌어도 DB snapshot으로 안전하게 복구되는 것이다.

## 구현 체크리스트

- [ ] aggregate별 revision column과 migration.
- [ ] 공통 `expectedRevision` request schema.
- [ ] 공통 `revision_conflict` response schema.
- [x] guild-scoped topic builder.
- [x] session/guild permission 기반 subscribe guard.
- [x] transaction과 같은 commit에 outbox event 기록.
- [x] outbox relay와 resource invalidate client store.
- [ ] aggregate별 single-flight autosave queue.
- [x] reconnect 후 subscribe + full refetch.
- [x] `RealtimeEditStateBar` 연결.
- [ ] 일반 엔트리 two-client test.
- [ ] 픽밴 two-client test.
- [ ] 경매 finalize 경쟁 test.
- [ ] Valkey 장애 복구 test.
