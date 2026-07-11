# Realtime Collaboration

작성일: 2026-07-11

## 목적

이 문서는 MonkeyBot의 동시 편집과 WebSocket 동기화에 대한 단일 기준이다.

다음 화면은 여러 운영자가 동시에 열 수 있다.

- 일반 내전 엔트리 편성
- 일반 내전 픽밴과 결과 입력
- 경매 팀장/포인트 설정
- 경매 draw, bid intent, 낙찰, 수동 배치
- 경매 match 라인업과 결과 입력

구현 중 판단이 충돌하면 이 문서를 우선한다. DB 구조는 `05-db-schema.md`, HTTP route와 topic 목록은 `routing/backend.md`, 화면 배치는 `routing/frontend.md`를 따르되 동시 편집 동작은 이 문서를 기준으로 한다.

## 결정 사항

```txt
DB                     = 영속 상태와 revision의 source of truth
HTTP API               = 모든 상태 변경과 권한 검증
WebSocket              = invalidate 전달
Valkey                 = API instance 간 Pub/Sub, presence, cache, 보조 lock
Frontend local state   = 아직 저장되지 않은 사용자 입력
```

필수 원칙:

- WebSocket message만으로 DB 원장 상태를 확정하지 않는다.
- Valkey lock만으로 낙찰, 라인업 확정, 결과 입력을 확정하지 않는다.
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
- presence 또는 distributed lock을 정합성 근거로 사용.
- offline mutation queue.

MVP의 충돌 정책은 `revision 검증 -> 409 Conflict -> 최신 snapshot 조회 -> 사용자 재적용`이다.

## Aggregate와 Revision

동시 편집 단위마다 하나의 aggregate revision을 가진다.

```txt
recruitment_entry_draft.revision
series_pickban_draft.revision
auction_tournament.revision
auction_match.revision
```

규칙:

- revision은 0 이상의 정수다.
- aggregate를 변경하는 transaction마다 정확히 1 증가한다.
- 실패하거나 rollback된 transaction은 revision을 증가시키지 않는다.
- audit log, updated_at, updated_by는 같은 transaction에서 기록한다.
- 같은 aggregate의 모든 GET 응답과 mutation 응답에 revision을 포함한다.

조회 응답 예시:

```json
{
	"id": "s-104",
	"revision": 42,
	"updatedAt": 1783750000,
	"updatedBy": "328516947210438656",
	"draft": {}
}
```

Mutation 요청 예시:

```json
{
	"expectedRevision": 42,
	"operation": {
		"type": "SET_PICK",
		"gameNumber": 2,
		"team": "BLUE",
		"slot": 3,
		"championId": "Ahri"
	}
}
```

성공 응답 예시:

```json
{
	"ok": true,
	"revision": 43,
	"updatedAt": 1783750012,
	"updatedBy": "328516947210438656"
}
```

충돌 응답 예시:

```json
{
	"error": "revision_conflict",
	"expectedRevision": 42,
	"currentRevision": 43,
	"updatedAt": 1783750008,
	"updatedBy": "954301284660912138"
}
```

## Mutation 단위

동시 편집 화면은 사용자가 수행한 최소 의미 단위로 mutation한다.

| 화면        | 허용 mutation 예시                              | 금지 방식                      |
| ----------- | ----------------------------------------------- | ------------------------------ |
| 엔트리 편성 | 참가자 1명을 특정 팀/라인으로 이동, 슬롯 비우기 | 전체 배정 JSON PUT 반복        |
| 픽밴        | 특정 game/team/slot의 pick 또는 ban 설정/해제   | 전체 시리즈 draft 덮어쓰기     |
| 경매 준비   | 팀장 1명 지정, 팀 포인트 변경                   | 전체 tournament 객체 저장      |
| 경매 입찰   | draw, bid intent, finalize bid, manual assign   | 브라우저가 낙찰 결과 직접 계산 |
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
8. 상태 변경, revision + 1, audit/checkpoint 기록
9. DB transaction commit
10. Valkey Pub/Sub invalidate publish
11. 새 revision을 HTTP response로 반환
```

주의:

- 6번이 실패하면 `409 Conflict`를 반환하고 아무 상태도 변경하지 않는다.
- DB commit 이후 Pub/Sub publish가 실패해도 commit을 rollback하지 않는다.
- publish 실패는 error log와 metric으로 기록한다.
- HTTP 성공 응답 자체가 현재 client를 동기화할 수 있어야 한다.
- 다른 client는 reconnect, window focus, 다음 invalidate에서 누락을 복구한다.

## WebSocket 계약

### 연결과 인증

- WebSocket은 기존 Discord OAuth session으로 인증한다.
- subscribe 요청마다 guild membership과 resource 조회 권한을 확인한다.
- client가 임의 guild 또는 다른 사용자의 topic을 구독하게 두지 않는다.
- session이 만료되거나 권한이 제거되면 해당 subscription을 종료한다.

### Topic 형식

```txt
guild:{guildId}:dashboard
guild:{guildId}:auction-dashboard
guild:{guildId}:recruitment:{recruitmentId}
guild:{guildId}:series:{seriesId}
guild:{guildId}:auction-recruitment:{recruitmentId}
guild:{guildId}:auction-tournament:{tournamentId}
guild:{guildId}:auction-match:{matchId}
guild:{guildId}:leaderboard:{role}
user:{discordId}
```

### Invalidate event

```json
{
	"type": "invalidate",
	"topic": "guild:123:auction-tournament:a-118",
	"resourceId": "a-118",
	"revision": 43,
	"occurredAt": 1783750012
}
```

규칙:

- event에 전체 aggregate 또는 민감한 payload를 넣지 않는다.
- 단일 resource topic은 `resourceId`와 `revision`을 포함한다.
- client는 현재 revision 이하의 중복 또는 역순 event를 무시한다.
- 현재 revision보다 큰 event를 받으면 HTTP GET으로 snapshot을 다시 조회한다.
- dashboard와 leaderboard 같은 collection topic은 revision 비교 없이 짧게 coalesce한 뒤 한 번 조회한다.
- WebSocket event의 도착 순서와 HTTP response의 도착 순서는 보장된다고 가정하지 않는다.

## Frontend 상태 머신

각 동시 편집 workspace는 아래 상태를 가진다.

| 상태           | 의미                               | 허용 동작                                        |
| -------------- | ---------------------------------- | ------------------------------------------------ |
| `loading`      | 최초 snapshot 조회 중              | 입력 금지                                        |
| `synced`       | server snapshot과 local state 일치 | 입력 가능                                        |
| `dirty`        | 아직 전송하지 않은 local 변경 존재 | 추가 입력 가능                                   |
| `saving`       | mutation 진행 중                   | 같은 대상의 중복 mutation 금지                   |
| `reconnecting` | WebSocket 재연결 중                | 일반 draft 입력 유지, 위험 command 보류          |
| `offline`      | WebSocket 연결 실패                | local 입력 유지, 최신 revision 필요 command 금지 |
| `conflict`     | server revision과 local 변경 충돌  | 충돌 해결 전 해당 대상 저장 금지                 |

`RealtimeEditStateBar`는 이 상태를 간결하게 표시한다.

```txt
synced       -> 연결 아이콘 + 저장됨
dirty        -> 연결 아이콘 + 저장 대기
saving      -> 연결 아이콘 + 저장 중
reconnecting -> 재연결 중
offline      -> 연결 끊김 + 재연결 액션
conflict     -> 변경 충돌 + 최신 상태 반영/로컬 입력 유지
```

## Autosave 규칙

Draft autosave는 다음 규칙을 따른다.

- 사용자 입력 후 400~800ms debounce한다.
- aggregate별 in-flight mutation은 최대 1개다.
- 저장 중 추가 입력은 local state에 유지하고 현재 요청 성공 후 다음 mutation으로 보낸다.
- 이전 HTTP 요청이 진행 중이라고 새 요청을 병렬 전송하지 않는다.
- 성공 응답의 revision을 즉시 현재 revision으로 갱신한다.
- `409 Conflict`가 오면 자동 재시도하지 않는다.
- 페이지 이탈 전 dirty 상태가 있으면 저장 완료를 기다리거나 이탈 경고를 표시한다.

전체 JSON autosave가 필요한 legacy endpoint는 신규 화면에서 사용하지 않는다. migration 기간에만 adapter 내부에서 허용하고 제거 계획을 기록한다.

## Invalidate와 로컬 입력

Invalidate 수신 시 local state 처리:

```txt
local = synced
  -> 최신 snapshot 조회
  -> revision과 화면 상태 교체

local = dirty 또는 saving
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
ws:{topic}                              # instance 간 Pub/Sub
guild:{guildId}:presence:{topic}        # 접속자 표시, 짧은 TTL
guild:{guildId}:lock:{resourceId}       # 중복 실행 완화, 짧은 TTL
guild:{guildId}:cache:{...}             # 재생성 가능한 cache
```

Valkey에만 저장하면 안 되는 것:

- 엔트리 배정 draft.
- 픽밴 draft.
- 경매 현재 단계와 현재 매물.
- 실제 bid와 낙찰 결과.
- 팀 포인트와 선수 배치.
- lineup lock과 game result.
- MMR 변경.

Valkey lock은 UX 보조 장치다. lock이 없거나 만료되어도 DB revision, state condition, unique constraint가 잘못된 두 번째 commit을 막아야 한다.

## 경매 내전 추가 규칙

경매는 일반 draft보다 경쟁 조건의 영향이 크므로 다음을 추가로 지킨다.

### Draw

- draw 대상과 tournament stage를 transaction 안에서 검증한다.
- 이미 active lot이 있으면 새 draw를 거부한다.
- draw 결과와 revision을 DB에 commit한 뒤 invalidate한다.

### Bid intent

- bid intent는 짧은 사용자 의사 표시일 수 있지만 server가 현재 lot과 팀 권한을 검증한다.
- 최종 낙찰 가격과 승자는 client intent 목록을 그대로 신뢰하지 않는다.
- bid intent를 Valkey에 두더라도 낙찰 확정에 필요한 최종 검증은 DB에서 다시 수행한다.

### Finalize bid

하나의 DB transaction에서 다음을 처리한다.

```txt
active lot 확인
expectedRevision 확인
winning team / bid 검증
남은 point 검증 및 차감
선수 team 배치
auction bid 원장 기록
revert checkpoint 기록
tournament revision 증가
```

동시 finalize 요청 중 하나만 성공해야 한다.

### Lineup과 결과

- lineup은 role별 mutation으로 저장한다.
- lineup lock은 5개 role 완성, 선수 중복, team membership을 transaction에서 검증한다.
- locked lineup의 revision을 기준으로만 game result를 입력한다.
- game 생성, MMR 변경, series/match score 변경은 하나의 transaction으로 처리한다.

## Presence 규칙

Presence는 협업 편의를 위한 정보이며 권한이나 lock이 아니다.

- 짧은 TTL을 사용한다.
- heartbeat가 끊기면 자동 만료한다.
- 같은 사용자의 여러 tab은 별도 connection으로 계산할 수 있다.
- presence가 비어 있어도 다른 사용자가 수정하지 않는다고 가정하지 않는다.
- 화면에는 최대 몇 명의 이름만 표시하고 나머지는 count로 표시한다.

## 관측 가능성

최소 로그:

- WebSocket connect/disconnect/reconnect.
- subscribe 거부와 권한 실패.
- revision conflict.
- mutation latency와 실패 원인.
- DB commit 후 invalidate publish 실패.
- 경매 finalize 경쟁 요청과 거부 사유.

권장 metric:

```txt
ws_connections
ws_reconnects_total
ws_publish_failures_total
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
- [ ] guild-scoped topic builder.
- [ ] session/guild permission 기반 subscribe guard.
- [ ] DB commit 이후 invalidate publisher.
- [ ] resource/collection invalidate client store.
- [ ] aggregate별 single-flight autosave queue.
- [ ] reconnect 후 subscribe + full refetch.
- [ ] `RealtimeEditStateBar` 연결.
- [ ] 일반 엔트리 two-client test.
- [ ] 픽밴 two-client test.
- [ ] 경매 finalize 경쟁 test.
- [ ] Valkey 장애 복구 test.
