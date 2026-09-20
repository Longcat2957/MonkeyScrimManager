# FastAPI migration 실행 기록

기준 계획: [18-fastapi-migration-plan.md](../18-fastapi-migration-plan.md).

## 현재 검증 기준

사용자 지시: frontend 연결을 제외하고 백엔드 구현·테스트로 성공 여부를 확인한다. 이에 따라 현재 단계는 격리 PostgreSQL과 테스트 대역을 통한 계약·권한·경쟁·오류 테스트, Ruff/mypy, Docker build/실행 검증을 완료 조건으로 사용한다. frontend check·build, 5173 proxy smoke, 기존 로컬 Axum reload는 실행하지 않으며 이전 proxy 연결 실패는 현재 blocker가 아니다.

## 열여섯 번째 구현 단위 — 일반 엔트리→경매 모집 전환 (2026-09-20)

- Browser 전용 `POST /api/guilds/{guild_id}/recruitments/{recruitment_id}/auction-recruitment`를 이관했다. 실제 method/path를 동결 Rust inventory와 대조했으며 **67/140개 구현, 73개 남음(API 68, Riot 5)**이다. 자동 HEAD는 별도 집계하지 않는다.
- 현재 ACTIVE 운영자 권한을 idempotency replay보다 먼저 확인한다. CSRF, source recruitment revision, CLOSED 상태, JOINED 10명, 모집 채널, 기존 active series 부재를 검사한다. 예상 시작 시간은 기존 RFC3339·기본 30분 후·KST 제목 규칙을 유지한다.
- 원본 시즌·BO·hardFearless·참가 순서를 보존한 CLOSED TEN_PERSON 경매 모집(revision 1), setup draft(revision 0), 팀 슬롯 2개(각 1000포인트)를 생성한다. 라인 미배정을 허용하며 배정된 참가자만 해당 시즌 MMR을 고정 저장한다. 미배정 참가자는 lane/MMR null 및 UNAVAILABLE을 저장한다. 원본 엔트리 배정과 revision은 유지한다.
- 원본 CONVERTED 전환, 기존 모집 메시지 UPDATE, 경매 모집 CREATE, 감사 로그, 두 invalidate event, idempotency 응답을 같은 transaction으로 저장한다. 전환된 원본 모집 GET은 기존 계약처럼 404이고 새 경매 모집 상세·준비 드래프트는 조회 가능하다.
- Rust/Python 양방향 재응답, 전체·부분·미배정 roster, MMR 보존, 동시 전환 단일 성공, 권한 회수 후 replay 거부, 입력·정원·상태·active series 오류, audit/realtime/notification 실패의 전체 rollback을 포함한 **신규 23개 테스트**를 추가했다.
- **기존 회귀 343개 + 신규 23개, 총 366개 통과.** 최초 전체 실행에서 신규 테스트 4개의 응답 필드명 기대값 오류를 발견해 수정하고 신규 파일 23개만 재실행했다. 백엔드 구현 변경은 없었으며 통과한 기존 회귀는 반복하지 않았다. Ruff/mypy/format/baseline 및 diff whitespace 검사 통과. Docker build와 실제 HTTP 복구→전환→상세→동일 요청 재응답, 원본 CONVERTED 저장·GET 404까지 검증했다. 격리 테스트 컨테이너는 정리했다.
- 다음 경매 범위는 브래킷·라인업·경기 결과·대회 종료/취소와 Bot 강제 취소다. Bot 강제 취소는 CONVERTED 대회의 경기/MMR rollback을 포함하므로 대회 취소 이관과 함께 처리한다. 실제 배포·Discord 발송·프런트엔드 연결은 수행하지 않았다.

## 열다섯 번째 구현 단위 — 경매 대회 생성·입찰 전체 흐름 (2026-09-15)

- 경매 대회 목록 GET·생성 POST·상세 GET·명령 POST의 **4개 operation**, 명령 내부의 **9개 입찰 동작**을 함께 이관했다. 등록된 method/path를 동결 Rust inventory와 대조했으며 **66/140개 구현, 74개 남음(API 69, Riot 5)**이다. 자동 HEAD와 개별 명령 종류를 별도 API로 중복 집계하지 않는다.
- 생성은 운영자 권한, CLOSED 모집, 정원·팀장·팀 이름·포인트·setup draft revision을 확인한다. 10인/2팀·20인/4팀, 팀장 roster slot 1, 초기 포인트 원장, 신청 당시 lane/MMR snapshot, 무작위 후보 순서와 네 개 bracket slot을 Rust처럼 저장한다. 모집 CONVERTED 전환, 메시지 UPDATE, 감사 로그, 두 invalidate event, idempotency 응답을 같은 transaction으로 처리한다.
- DRAW_NEXT, RESHUFFLE_CANDIDATES, MARK_UNSOLD, AWARD_BID, ASSIGN_CURRENT, ASSIGN_UNSOLD, RESTORE_UNSOLD, REVERT_LAST, RESET_TO_INITIAL을 구현했다. 낙찰은 0포인트도 허용하고 수동 배정은 포인트를 차감하지 않는다. 유찰 복원은 drawCount를 증가시키지 않으며 모든 팀이 5명이 되면 BRACKET_SETUP으로 전환한다.
- 되돌리기는 원래 action과 lot·roster·포인트 원장을 함께 확인하고 낙찰 포인트를 역분개한다. 현재 추첨 대상 복원, 수동 배정 해제, 전체 팀 완성 후 BIDDING 복귀, 최근 action/되돌리기 capability의 Rust 동작을 보존한다. 경기 기록이 있으면 수정·초기화·되돌리기를 막는다.
- 상세의 현재 대상 프로필, lane별 MMR/전적, 챔피언 선호, 내부 경기 기록과 유효한 Riot cache의 전적·티어·아이콘을 반환한다. 손상된 Riot cache는 Rust처럼 제외한다. 목록의 Browser/Bot 인증, season/state/limit/cursor 계약, 팀·참가자 수와 상태별 href를 유지한다. Riot 외부 조회 API 이관을 의미하지 않는다.
- **기존 초기화의 runtime 권한 제약을 보존했다.** RESET_TO_INITIAL은 auction_point_ledger와 auction_actions를 DELETE하지만 실제 runtime grants는 이를 금지한다. 현재 Rust와 FastAPI 모두 500으로 실패하며 전체 rollback한다. 운영 권한/원장 정책을 변경하지 않았다. 초기화 알고리즘은 격리 테스트에서만 임시 DELETE 권한을 부여하고 finally에서 회수하여 양방향 Rust 호환성을 확인한다. Docker에서는 실제 권한의 실패·rollback을 검증한다.
- 신규 테스트는 Rust/Python 양방향 생성·9개 명령 재응답 및 상세 비교, 되돌리기 6종, 10인/20인 완료, 동시 revision/생성 경쟁, 운영자 권한 회수 후 replay 거부, 잘못된 명령·대상·팀·포인트, audit/outbox 실패 rollback, MMR 고정값과 Riot cache 검증, 경기 기록의 드래프트 잠금을 포함한다.
- **신규 31개, 전체 백엔드 테스트 343개 통과.** Ruff/mypy/format/baseline 및 diff whitespace 검사 통과. Docker build와 실제 HTTP 대회 생성→목록/상세→입찰 명령→동일 요청 재응답, 초기화 권한 오류의 rollback 검증까지 통과했다. 격리 테스트 컨테이너를 정리했으며 실제 배포·Discord 발송·프런트엔드 연결은 수행하지 않았다.
- 다음 경매 범위는 일반 엔트리→경매 전환, Bot 강제 취소, 브래킷·라인업·경기 결과·대회 종료/취소다. 관리/통계·Riot·WebSocket/worker 및 운영 전환도 남아 있다.

## 열네 번째 구현 단위 — 경매 모집 전체 흐름 12개 API (2026-09-15)

- 경매 모집 목록/상세 GET, 생성 POST, 모집 취소 DELETE, 참가 추가 POST/취소 DELETE, 마감/재개/복구 POST, 준비 드래프트 GET/PATCH, 참가 확정 DM POST의 **12개 operation**을 한 묶음으로 이관했다. 등록된 method/path를 동결한 Rust inventory와 대조했으며 **62/140개 구현, 78개 남음(API 73, Riot 5)**이다. 자동 HEAD는 별도 집계하지 않는다.
- 기존 Rust의 10인/2팀·20인/4팀, BO1/3/5 및 semifinal 기본값, hardFearless 기본값, KST 예상 시작 제목과 Browser/Bot 인증 분기를 유지한다. 일반 참가자는 모집을 생성할 수 있고, Browser 참가 변경/마감/재개/팀장 설정/DM은 운영자 권한을 요구한다. 취소/복구는 활성 생성자 또는 운영자에게 허용한다.
- 참가 시 신청 lane의 시즌 MMR을 고정 저장하고 누락된 MMR만 1500으로 초기화한다. 참가 취소/재참가 시 recruitment와 setup draft revision을 함께 증가시킨다. 참가 취소는 해당 팀장 지정만 해제하고 팀 이름·포인트는 유지한다. 재개는 recruitment revision만 증가시키며 팀장/이름/포인트와 draft revision을 보존한다.
- 준비 드래프트는 CLOSED에서만 조회/수정한다. 정원, draft expectedRevision, 팀 수/연속 index, trim 후 UTF-8 64바이트 이름, 양수 포인트, 중복 없는 현재 참가 팀장을 검증한다. 설정 저장은 draft revision과 audit만 변경하며 recruitment revision/updatedAt, invalidate, Discord 알림을 추가하지 않는다.
- CANCELLED 상세는 Rust처럼 반환한다. 복구는 closed_at에 따라 OPEN/CLOSED로 돌아가고 message_id를 비운다. CONVERTED 상세는 연결된 경매 대회 draft 링크를 반환하며 actionable 목록에서는 전환된 모집과 유효한 대회가 연결된 모집을 제외한다.
- 생성·참가 변경·lifecycle의 audit/invalidate/notification/idempotency를 같은 transaction으로 저장한다. Bot 참가 변경도 Rust처럼 UPDATE 알림을 적재한다. 경매 메시지는 schemaVersion 2 및 auctionRecruitmentId를 사용한다. DM은 현재 JOINED Discord 사용자만 대상으로 하며 더미 제외, revision/수신자별 중복 제거, payload 충돌 시 전체 rollback을 보존한다.
- Rust/Python 양방향 idempotency replay, 10인 전체 흐름, 20인/4팀 설정, 권한 검사와 replay 순서, 마지막 자리/동일 revision 경쟁, MMR 고정·재가입, 취소/재개 후 설정 보존, 목록 pagination/전환 링크, 입력·상태·의존성 오류와 audit/outbox 실패 rollback을 포함한 **신규 35개 테스트**를 추가했다.
- **전체 백엔드 테스트 312개 통과**, Ruff/mypy/format/baseline 및 diff whitespace 검사 통과. Docker build와 실제 HTTP 12개 API 흐름, 생성 알림 claim→typed ACK→메시지 연결, 요청 재응답 검증 통과. 격리 테스트 컨테이너는 정리했다. 실제 Discord 발송은 수행하지 않는다.
- 다음 경매 범위는 일반 엔트리→경매 전환, Bot 강제 취소, 대회 생성·입찰 draft·브래킷·라인업·경기 결과다. 관리/통계·Riot·WebSocket/worker 및 운영 전환도 남아 있다. 기존 프런트엔드와 로컬 Axum 실행 인스턴스는 이번 검증 대상이 아니다.

## 열세 번째 구현 단위 — Discord 카탈로그·길드 설정·시즌 관리 (2026-09-15)

- Bot 전용 `PUT /internal/guilds/{guild_id}/discord-catalog`, 운영자 전용 `GET/PUT /api/guilds/{guild_id}/settings`, `GET/POST /api/guilds/{guild_id}/seasons`의 **5개 operation**을 이관했다. 실제 등록 operation **50/140개**, 남은 operation **90개(API 85, Riot 5)**다.
- Discord 카탈로그는 역할 250개/채널 500개 상한, 중복 ID·everyone 역할·음수 position 거부, 이름 trim/100자 제한을 보존한다. 길드 config를 잠근 뒤 역할·채널·동기화 시각을 한 transaction으로 교체한다. 설정 조회는 managed 역할 제외, 역할/채널 정렬과 nullable parentName을 그대로 반환한다.
- 설정 수정은 ACTIVE 회원의 현재 운영자 역할을 잠금 상태에서 검사한다. expectedRevision 충돌 응답에 최신 revision/updatedAt/updatedBy를 포함한다. 모집 채널은 필수이며 변경한 역할·채널만 현재 카탈로그에서 검증한다. 카탈로그에서 삭제된 기존 선택 유지와 optional 채널 해제를 허용한다. 같은 값으로 수정해도 revision과 감사 로그를 갱신하며 별도 idempotency/outbox는 추가하지 않는다.
- 시즌 조회는 repeatable-read transaction으로 현재/완료 시즌, 완료된 일반·경매 대회 수, 확정 경기 수·최근 경기 시각·중복 제거 참가자 수를 집계한다. 운영자가 아니면 403, 길드/ACTIVE 회원이 없으면 404, 진행 중 시즌이 없으면 409를 반환한다. 서비스 관리자 우회는 추가하지 않는다.
- 시즌 전환은 운영자 검사→idempotency→길드별 rotation advisory lock→현재 시즌 잠금 순서를 보존한다. 이전 시즌 CLOSED, 새 시즌 ACTIVE, 감사 로그와 재응답 저장을 원자적으로 처리한다. 이름은 저장 시 trim하지만 요청 hash에는 원문을 사용한다. 기존 모집·경기·MMR은 이전 시즌에 남으며 이후 신규 모집부터 새 시즌을 사용한다.
- 실제 Rust repository와 카탈로그/설정/시즌 응답을 비교하고 시즌 전환의 양방향 재응답을 검증했다. 동시 설정 수정·시즌 전환, 운영자 변경 후 접근 거부, 재응답 전 권한 검사, 잘못된 선택·입력, 카탈로그/감사 로그 저장 실패 시 전체 rollback, 손상된 재응답 거부를 포함한다.
- **전체 백엔드 테스트 277개 통과**, Ruff/mypy/format/baseline 통과. Docker build 및 실제 HTTP 카탈로그 동기화→설정 조회/수정→시즌 조회/전환→동일 요청 재응답 통과. 격리 테스트 컨테이너 정리 완료.
- 기존 시리즈 결과 알림의 schemaVersion이 Rust 상수와 달랐던 부분도 **2**로 수정했다. 전체 회귀 실행 이후 해당 변경은 시리즈 교차 테스트 1개를 별도로 재실행하고 Docker HTTP 검증까지 통과했다. 재실행한 테스트는 총 개수에 중복 집계하지 않는다.
- 경매 업무 API·회원 관리/통계·서비스 상태·Riot·WebSocket/relay 및 운영 전환은 계속 남아 있다.

## 열두 번째 구현 단위 — Bot 알림 전달·메시지 연결 확인·heartbeat (2026-09-15)

- `POST /internal/heartbeat`, `POST /internal/notifications/claim`, `ack`, `nack`, `recruitment-message/ack`, `auction-recruitment-message/ack`의 **6개 operation**을 이관했다. 실제 등록 operation **45/140개**, 남은 operation **95개(API 90, Riot 5)**다.
- Bot service token 인증 후 엄격한 JSON 계약을 검사한다. claim은 기본 20개/최대 50개, 30초 lease, UUID token, 동일 nonce 재사용, SKIP LOCKED 선점을 보존한다. 같은 일반·경매 모집의 CREATE/UPDATE/DELETE 알림은 앞선 미완료 알림을 추월하지 않는다. 만료된 8회 시도 알림은 MAX_ATTEMPTS로 정리한다.
- 일반 ack는 모집 CREATE를 거부한다. 모집 전용 ack는 전달 확인과 channel/message 연결을 같은 transaction으로 처리하고, 최초 연결에만 revision을 증가시킨다. 이미 모집 상태가 바뀌었다면 최신 UPDATE 또는 CANCELLED의 DELETE 알림을 적재한다. 경매 payload schemaVersion 1/2를 모두 수용하고 후속 payload는 Rust처럼 2로 생성한다.
- nack는 HTTP 429의 retryAfterMs, 전송 실패·5xx의 UUID 기반 지수 지연/jitter, 4xx 최종 실패, DELIVERY_UNKNOWN 및 최대 시도 횟수를 보존한다. 만료·이전된 lease는 409를 반환하며 재확인은 성공으로 간주하지 않는다.
- heartbeat는 instance별 version/commit/최근 수신 시각을 갱신하면서 최초 started_at을 보존한다. 알림 요청의 UUID 형식도 실제 Rust uuid parser가 허용하는 네 가지 형식으로 제한했다.
- 실제 Rust repository와 양방향 claim/ack, 재시도 후 재선점, 동일 알림의 재시도 지연을 비교했다. 동시 선점·잠긴 행 건너뛰기·알림 순서, 만료 token 거부, 잘못된 failure 입력, 메시지 연결 충돌, payload/revision 오류 및 후속 알림 중복 충돌의 전체 rollback을 검증했다.
- **회귀 테스트 246개 + UUID 계약 테스트 9개, 총 255개 통과**. 최종 UUID 형식 변경은 별도 계약 테스트로 확인했으며 변경 후 Ruff/mypy와 Docker build/HTTP 검증도 통과했다. Docker에서 기존 일반 내전 흐름과 함께 heartbeat→claim→일반/경매 메시지 연결 확인→nack→재선점→ack를 실행했다. 격리 테스트 컨테이너 정리 완료.
- 이번 범위는 Bot 소비용 백엔드 API다. 실제 Discord 발송은 수행하지 않았다. `/api/admin/status`, 경매 업무 API·관리/통계·Riot·WebSocket/relay 및 운영 전환은 계속 남아 있다.

## 열한 번째 구현 단위 — 시리즈 조회·픽밴·경기/MMR·종료·되돌리기 (2026-09-15)

- 시리즈 목록 GET, 상세 GET, 결과 GET, 픽밴 PATCH, 경기 확정 POST, 마지막 경기 DELETE, 조기 종료 POST, 모집 복귀 POST, 시리즈 DELETE의 **9개 operation**을 함께 이관했다. 등록 operation **39/140개**, 남은 operation **101개(API 96, Riot 5)**다.
- 목록은 ACTIVE/COMPLETED 상태, 점수와 결과 href, updatedAt/UUID cursor 경계를 보존한다. 상세·결과는 repeatable-read snapshot으로 고정 참가자, 배정 lane, MMR, Riot 티어, 챔피언 선호와 운영자 capabilities를 반환한다. 진행 중 상세와 완료 결과의 상태별 404/409 구분도 Rust를 따른다.
- 픽밴의 진영 선택, 픽·밴 설정/해제, 일괄 변경을 구현했다. 진영 선택 선행 조건, 챔피언 중복, 하드 피어리스, 현재 세트 및 expectedRevision을 검사한다. 픽밴은 기존 Rust처럼 idempotency key를 요구하지 않는다.
- 경기 확정은 입력 snapshot, 경기 참가자·밴, lane별 MMR 및 append-only ledger, 시리즈 점수·종료 상태, audit/outbox/idempotency를 한 transaction으로 저장한다. BO1 자동 종료와 BO3 승수 종료, BO3/BO5 조기 승리·무승부를 검증했다. 완료 시 결과 알림 snapshot을 적재한다.
- 마지막 경기 취소는 MMR 역분개 후 경기를 VOIDED, 입력을 SUPERSEDED로 전환하고 시리즈를 다시 연다. 이후 MMR 변경이 있으면 안전하게 거부한다. 시리즈 모집 복귀·삭제는 확정 경기를 역순으로 모두 취소하고 원본 모집을 각각 CLOSED/CANCELLED로 변경하며 기존 엔트리 배정을 보존한다.
- 실제 Rust repository와 상세·결과 snapshot을 비교하고 경기 확정·취소·조기 종료·모집 복귀·삭제의 양방향 idempotency 재응답을 검증했다. 동시 revision 충돌, 권한/CSRF 거부, 잘못된 픽밴 입력, audit 실패 시 경기·MMR 전체 rollback도 검증했다.
- **백엔드 테스트 216개 통과**, Ruff/mypy/format/baseline 통과. Docker build 및 실제 HTTP 시리즈 목록→상세→픽밴→경기 확정→조기 종료→결과→경기 취소→모집 복귀→재생성→시리즈 삭제 통과. 격리 테스트 컨테이너 정리 완료.
- 경매·관리/통계·Riot·WebSocket/worker 및 운영 전환은 계속 남아 있다. Discord 알림은 적재까지 검증했으며 실제 발송 worker 이관 완료를 의미하지 않는다.

## 열 번째 구현 단위 — 메시지 연결·참가 확정 DM·시리즈 생성 (2026-09-15)

- Bot 전용 `PUT .../recruitments/{id}/message`, 세션 운영자 전용 `POST .../recruitments/{id}/participant-confirmation-dms`, `POST /api/guilds/{guildId}/series`를 이관했다. 등록 operation **30/140개**, 남은 operation **110개(API 105, Riot 5)**다.
- 메시지 연결은 OPEN/CLOSED에서 생성자 또는 Bot actor의 운영자 역할을 검사한다. recruitment revision만 증가시키고 audit/invalidate를 저장한다. 기존 Rust처럼 연결 자체로 새 Discord notification을 적재하지 않는다.
- 참가 확정 DM은 CLOSED 모집의 JOINED Discord 참가자만 대상으로 하며 더미 사용자는 제외한다. 모집 revision/수신자별로 중복을 제거하고 queuedRecipientCount/alreadyQueuedRecipientCount를 반환한다. 알림 중복 키의 channel/type/payload가 충돌하면 Rust처럼 전체 요청을 rollback한다. recruitment/draft revision 및 invalidate event는 추가하지 않는다.
- 시리즈 생성은 운영자 권한을 먼저 검사하고 기존 idempotency 응답을 반환한다. 모집 revision, CLOSED 상태와 10명 전원/양 팀 5개 lane 배정을 확인한 뒤 기존 lane MMR을 잠그고 누락된 MMR만 1500으로 초기화한다. 참가자별 mmr_at_lock과 CAPTURED origin을 저장한다.
- series 번호 발급, IN_PROGRESS 시리즈 생성, 모집 CONVERTED 전환, 모집 메시지 UPDATE, 설정된 채널의 ENTRY_CONFIRMATION_CREATE, audit 및 두 invalidate event를 같은 transaction으로 저장한다. 응답은 Rust와 같은 201 및 pick-ban href다. draft revision과 기존 MMR 값/전적/revision은 변경하지 않는다.
- Rust/Python 양방향 요청 재응답, DM 중복 및 revision 변경 후 재요청, 더미 제외, MMR 고정 저장, entry 채널 유무, 동시 시리즈 생성 단일 성공, 알림 payload 충돌, audit 저장 실패 시 모집·MMR·번호·알림 전체 rollback을 검증했다.
- **백엔드 테스트 199개 통과**, Ruff/mypy/format/baseline 통과. Docker build 및 실제 HTTP 메시지 연결→마감→DM 적재→시리즈 생성→동일 요청 재응답 통과. 격리 테스트 컨테이너 정리 완료.
- DM/채널 알림은 outbox 적재까지 검증했다. 실제 Discord 발송 worker는 아직 이관 전이며 테스트에서 외부 메시지를 발송하지 않는다. 시리즈 생성 이후 픽밴·경기 결과/MMR·되돌리기·종료와 경매·관리/통계·Riot·WebSocket/worker 이관은 남아 있다.

## 아홉 번째 구현 단위 — 선호 lane 변경·엔트리 편집·초기화 (2026-09-14)

- Bot 전용 `PUT /api/guilds/{guildId}/recruitments/{id}/members/{userId}/preferred-lanes`, 세션 운영자 전용 `PATCH .../entry-draft`, `POST .../entry-draft/reset`을 이관했다. 실제 등록 operation **27/140개**, 남은 operation **113개(API 108, Riot 5)**다.
- 선호 lane은 OPEN 모집의 JOINED 참가자에게만 설정하며, Bot 본인 또는 운영자 권한을 검사한다. 중복 없이 1~5개인 입력 순서를 보존하고 recruitment/draft revision을 함께 증가시킨다. 기존 Rust처럼 Bot 경로에는 Browser notification을 추가하지 않는다.
- 엔트리 변경은 CLOSED에서 draft expectedRevision을 검사한다. 배정된 참가자를 점유 슬롯으로 옮기면 교환하고, 미배정 참가자를 넣으면 기존 점유자를 미배정으로 돌린다. null 또는 생략된 slot은 배정을 해제한다.
- 편집·초기화는 draft revision만 증가시키고 recruitment의 updated_at/updated_by를 갱신한다. audit와 draft invalidate event를 원자적으로 적재하고 일반 모집 invalidate/Discord notification은 추가하지 않는다. 빈 엔트리 초기화도 Rust처럼 revision을 증가시킨다.
- 기존 마감·재개와 공유하는 transaction/idempotency 처리에 편집·초기화를 연결했다. Bot 인증 검사도 기존 생성/참가 처리와 공유한다.
- Rust/Python 양방향 요청 재응답, 슬롯 교환·해제·점유자 이동, 초기화, 동시 revision 경쟁, 동일 key의 다른 요청 거부, 재응답 이전 운영자 권한 검사, outbox 저장 실패 시 전체 rollback 테스트 통과.
- **백엔드 테스트 186개 통과**, Ruff/mypy/baseline 통과. Docker build와 실제 HTTP 엔트리 편집→초기화→재개→Bot 선호 lane 변경 검증 통과. frontend 연결·기존 Axum 교체·운영 배포는 수행하지 않았다.
- 남은 일반 내전 범위에는 Bot 메시지 연결, 참가 확정 DM, 시리즈 생성·픽밴·경기 결과/MMR·되돌리기·종료가 있다. 경매, 관리/통계, Riot, WebSocket/worker와 운영 전환도 계속 이관 대상이다.

## 여덟 번째 구현 단위 — 일반 모집 마감·취소·복구·재개와 엔트리 조회

- 현재 Rust 동작으로 마감 POST, 모집 취소 DELETE, 복구 POST, 재개 POST, 엔트리 draft GET을 이관했다. 등록된 명시적 operation **24/140개**, 남은 operation **116개(API 111, Riot 5)**다. HEAD는 별도 집계하지 않는다.
- 마감은 정원이 찬 OPEN 모집만 허용하고 recruitment revision만 증가시킨다. 취소는 생성자/운영자에게 허용하며 기존 엔트리 배정을 보존한다. CANCELLED 일반 모집 상세는 계속 200이며 엔트리 조회는 Rust처럼 OPEN/CLOSED만 노출한다.
- 복구는 기존 closed_at 유무에 따라 OPEN/CLOSED로 돌아가며 message_id를 비우고 CREATE notification을 적재한다. 재개는 CLOSED 모집을 OPEN으로 돌리고 recruitment/draft revision을 모두 증가시키며 기존 배정을 보존한다.
- 재개의 expectedRevision은 Rust처럼 recruitment revision을 검사한다. 재개는 권한 검사 후 idempotency 재응답, 나머지 lifecycle은 idempotency 재응답 후 권한 검사를 수행하는 기존 순서도 유지했다. 마감의 Browser/Bot 인증과 session 전용 취소·복구·재개 분기를 보존했다.
- 엔트리 snapshot은 repeatable-read transaction에서 참가자·선호 lane·MMR·Riot 티어·챔피언·배정·상대 전적·capabilities를 조회한다. MMR 선택 순서(배정 lane → main lane → 첫 선호 lane → 기본값), 챔피언 정렬 및 lane 간 중복 보존을 Rust repository 응답과 직접 비교했다.
- Rust/Python 양방향 lifecycle 재응답, CLOSED 취소/복구, OPEN 취소/복구, 재개 후 배정 보존, 동시 취소의 단일 반영, Bot 운영자 거부, 알림 적재 실패 시 전체 rollback 검증을 추가했다.
- **백엔드 테스트 172개 통과**, Ruff/mypy/baseline 통과. Docker build 및 실제 HTTP 생성→참가→참가 취소→모집 취소→복구→마감→재개→엔트리 조회 통과. 테스트 컨테이너는 종료·정리했다.
- 다음 남은 일반 모집 범위는 선호 lane 변경, 엔트리 편집·초기화, 시리즈 생성과 이후 경기 흐름이다. 경매·관리/통계·Riot·WebSocket/worker와 운영 전환도 아직 완료되지 않았다.

## 일곱 번째 구현 단위 — 일반 모집 참가·취소

- 참가 추가 POST와 참가 취소 DELETE를 Browser/Bot 양쪽으로 이관했다. 명시적 operation 총 **19개**.
- Browser는 ACTIVE 운영자, Bot은 ACTIVE 본인 또는 actor role snapshot의 운영자 권한을 검사한다. 서비스 관리자 우회는 추가하지 않는다.
- 모집 OPEN 상태, target 정원, 중복 참가, 미참가 취소, inactive/없는 대상 회원 오류를 보존한다. 참가 취소는 entry assignment만 삭제하고 preferred lane은 보존한다.
- recruitment와 entry draft revision을 함께 증가시키고 audit/outbox/idempotency를 원자적으로 저장한다. Browser는 Discord message가 bind된 경우 UPDATE notification도 저장하며 Bot 경로는 기존처럼 별도 queue update를 추가하지 않는다.
- Rust가 추가한 참가 요청을 Python으로 재응답하고, Python이 취소한 참가 요청을 Rust로 재응답하는 검증 통과. 마지막 한 자리 동시 참가 요청에서 한 건만 성공하는 테스트도 통과했다.
- 인증 분기·JSON Value 파싱·idempotency key 검증과 모집 snapshot을 기존 생성/상세 경로와 공유하도록 추출했다. 전체 backend 테스트 **159개 통과**, Ruff/mypy/baseline과 Docker build·실제 생성→재응답→상세→참가→취소 검사 통과.
- 모집 자체 취소/복구, 선호 lane 변경, 마감/재개, 엔트리 편성·시리즈 및 이후 migration 단계는 남아 있다. 이 단계의 참가 취소는 모집 자체 취소와 구분한다.

## 여섯 번째 구현 단위 — 일반 모집 생성

- Browser/Bot `POST /api/guilds/{guild_id}/recruitments`를 현재 Rust 흐름대로 이관했다. 명시적 operation 총 **17개**.
- Authorization header가 있으면 Bot 인증을 선택하며 session으로 fallback하지 않는다. Browser는 CSRF와 browser idempotency key, Bot은 service token과 interaction snowflake를 검증한다.
- 길드/config 잠금 → advisory idempotency lock → 기존 응답 재사용 → actor/member 검사 → season/counter → 모집·엔트리 생성 → audit/outbox/notification → idempotency 완료를 한 transaction으로 처리한다. Bot actor 동기화와 3회 deadlock 재시도도 포함한다.
- Rust repository가 생성한 요청을 Python이 재응답하고, Python이 생성한 요청을 Rust가 재응답하는 양방향 테스트를 통과했다. 나노초/UTC offset을 포함한 요청 hash 저장 형식을 보존한다.
- backend 테스트 **150개 통과**, Ruff/mypy/baseline 통과. Docker test fixture의 필수 guild config 값을 보완한 뒤 이미지 build 및 실제 생성→동일 요청 재응답→상세 조회도 통과했다.
- `scripts/test-fastapi.sh --docker-only`는 이미 통과한 pytest를 반복하지 않고 격리 migration/runtime grant와 Docker 검증을 실행한다.
- Discord 발송 worker는 아직 이관 전이다. 생성 테스트는 원자적인 notification 적재까지만 검증하며 실제 메시지는 발송하지 않는다.

## 다섯 번째 구현 단위 — Axum 동작 기준 확정·모집 상세

- 사용자 확정 지시: 현재 Rust 구현 그대로 이관한 뒤 별도 작업으로 수정한다. 문서와 구현의 불일치는 이관 중 동작 변경 근거로 사용하지 않는다.
- 일반 모집 상세 GET/HEAD 추가. OPEN/CLOSED/CANCELLED는 200, CONVERTED/없는 모집은 404이며 ACTIVE guild/member 권한과 참가자·선호 lane 순서를 보존한다. 명시적 operation 총 **16개**.
- OAuth rate key의 임의 변경을 제거하고 Rust의 X-Real-IP → CF-Connecting-IP → unknown 동작을 복원했다.
- backend 테스트 **133개 통과**, Ruff/mypy/baseline 및 Docker 이미지 build·HTTP smoke 통과. frontend는 검증 범위에서 제외한다.
- 전체 이관은 진행 중이며 일반 모집 mutation과 이후 workflow는 아직 남아 있다.

## 네 번째 구현 단위 — 일반 모집 목록

- `GET /api/guilds/{guild_id}/recruitments?state=actionable`을 추가했다. 명시적 operation은 총 **15개**다.
- ACTIVE guild/member 권한, OPEN/CLOSED 및 미전환 모집 필터, 유효한 series가 연결된 모집 제외, JOINED 참가/배정 수와 상태별 href를 보존한다.
- updatedAt/UUID 역순 페이지네이션과 Rust의 base64url JSON cursor를 사용한다. nanosecond 정수 직렬화와 PostgreSQL microsecond 변환에 float를 사용하지 않는다.
- 실제 PostgreSQL에서 동일 시각의 UUID 경계, 탈퇴 참가자, 취소/삭제된 series 재노출, 취소/전환 모집 제외, 잘못된 query와 권한 거부를 검증했다.
- backend 테스트 **125개 통과**, Ruff/mypy/baseline 및 Docker build/HTTP smoke 통과. 임시 container/volume 정리 완료.
- 계약 기준 확정: 사용자 지시에 따라 현재 Rust 구현을 그대로 따른다. CANCELLED 상세는 200 snapshot을 반환하며, 문서의 404 설명으로 동작을 변경하지 않는다.
- 목록의 state는 동결한 Rust handler가 지원하는 actionable만 이관했다. routing 문서에 함께 적힌 active/completed는 현재 일반 모집 handler에도 구현되어 있지 않다.
- 일반 모집 생성·참가·마감·엔트리·series mutation, realtime/notification worker와 이후 단계는 아직 이관 대상이다.

## 세 번째 구현 단위 — 프로필·개인정보·챔피언 선호

- 공개 사용자 프로필, 개인정보 설정 GET/PUT, 챔피언 선호 GET/PUT을 구현했다. 명시적 operation은 총 14개다.
- 개인정보 설정이 없는 사용자에 대한 기본값, 공개 프로필 비공개 전환, ACTIVE guild/member만 집계하는 공개 projection을 보존한다.
- 챔피언 목록은 사용자 행 잠금으로 모든 lane 변경을 직렬화한다. 순서 보존, 빈 목록 초기화, 다른 lane 유지, 전체 snapshot 반환, 저장 불변식 위반 시 rollback을 검증했다.
- 엄격한 JSON parser를 기본 길드·개인정보·챔피언 변경에서 공유하며 인증/JSON/CSRF 검증 순서를 유지한다.
- backend 테스트 **110개 통과**, Ruff/mypy/baseline 및 Docker build·실제 HTTP smoke 통과. 외부 Discord와 frontend 연결 없이 검증했다.
- P2의 나열된 API 구현은 완료했다. 운영 양방향 전환·전체 인증 설정·proxy quota 검증은 후속 운영 gate에 남는다. 다음 구현 대상은 P3 일반 내전 workflow다.

## 두 번째 구현 단위 — OAuth·CSRF·guild context

- OAuth start/callback, state/PKCE, login transaction, session rotation, CSRF/logout 구현.
- 길드 목록/context, active membership·operator role, 현재 season, 기본 길드 변경 구현.
- Python API의 명시적 operation은 health/readiness/me를 포함해 9개다. GET의 implicit HEAD는 별도 집계하지 않는다.
- backend 테스트 **90개 통과**. 실제 Rust AuthService를 사용하는 PKCE 양방향 복호화와 session/CSRF hash 호환, Rust가 만든 OAuth attempt를 Python callback으로 완료하는 흐름을 포함한다.
- 동시 callback 1회 성공, OAuth 재사용·만료·변조 거부, provider 오류 안전화, login identity conflict 전체 rollback, session rotation/logout, CSRF 중복 header/cookie 거부, tenant/role/default guild 경계를 검증했다.
- Ruff, mypy, baseline 검사 및 `monkeybot-api-python:test` Docker 이미지 build/HTTP 검증 통과. 검증 후 임시 container/volume 정리 완료.
- 실제 Discord는 호출하지 않았다. HTTPX MockTransport로 token/user 응답을 검증하고, 실제 Docker에서는 provider 호출 없이 OAuth authorization denial/replay를 검사한다.
- 당시 OAuth rate limit의 key를 연결 peer로 변경했으나, 이후 사용자 지시로 기존 Rust처럼 X-Real-IP → CF-Connecting-IP → unknown 순서로 복원했다. 이관 과정에서 정책을 개선하지 않는다.

이 시점에는 profile/preferences/privacy가 남아 있었으며, 위 세 번째 구현 단위에서 이관했다. 전체 migration 또는 운영 배포 완료를 뜻하지 않는다.

## 첫 구현 단위 기록 — 2026-09-13

Docker 배포 구조를 유지한다. 현재 작업은 별도 candidate이며 main/dev API 교체 단계가 아니다.

| 단계  | 상태         | 근거·남은 작업                                                                                                                                 |
| ----- | ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| P0    | 진행 중      | working tree source hash, 명시적 operation 140개(API 135, Riot 5), SQL migration 25개 동결. consumer/권한/DTO별 matrix와 운영 기준 측정은 남음 |
| P1    | 진행 중      | uv/Python/Docker, HTTP 오류·request ID, DB pool·readiness, 격리 PostgreSQL 테스트 구현. WS/worker·전체 설정·drain 동등성은 남음                |
| P2    | 첫 조회 구현 | 기존 session 저장 형식으로 `/api/me` 조회. OAuth·CSRF·guild context는 남음                                                                     |
| P3~P9 | 미착수       | workflow, Riot, realtime, Alembic, 운영 전환·안정화·Rust 제거                                                                                  |

operation 수는 method/path 단위이며 Axum GET의 implicit HEAD는 별도 집계하지 않는다. `/ws` upgrade도 목록에 포함한다. fallback/환경 flag는 별도 계약 검증 대상이다.

이전부터 수정 중이던 Axum·Web·OpenAPI 파일은 이 작업에서 수정하지 않았다. baseline은 해당 미커밋 변경을 포함한 source hash를 기록한다. 이미 동결한 Rust 계약이 바뀌면 `scripts/fastapi-baseline.py`가 drift를 알린다.

## 검증 범위

- `scripts/test-fastapi.sh`: isolated Docker PostgreSQL → 기존 SQLx migration → runtime grant → Python test → Docker build → 실제 HTTP health/readiness/401/session 조회 → container 정리.
- Ruff와 mypy, lock 일관성, baseline drift 검사.
- candidate 이미지의 비특권 UID, read-only filesystem, 기존 세션 조회 확인.

첫 구현 당시 결과:

- Python 테스트 26개, Ruff, mypy, baseline 검사 통과.
- `monkeybot-api-python:test` 이미지 build 및 실제 Docker HTTP smoke 통과. 테스트 container/volume 정리 완료.
- Web/Bot `pnpm check`, `pnpm lint`, `pnpm test`, `pnpm build` 통과.
- candidate Compose config 검증 통과. main/dev 배포 및 image push는 수행하지 않음.
- 기존 `scripts/local-api-reload.sh '/api/me'` 실행 시 DB 터널이 내려가 있어 시작 실패. 기존 `scripts/vps-db-tunnel.sh`로 복구 후 Axum readiness 200, direct `/api/me` 401 확인.
- 당시 `5173` frontend가 연결을 받지 않아 proxy smoke는 HTTP 000으로 실패. frontend dev server는 시작하지 않았다. 이후 사용자 지시로 현재 backend 완료 조건에서 proxy 검증을 제외했다.

Docker runtime 검증과 기존 frontend proxy 통합은 구분한다. 전체 migration 전까지 현재 운영/개발 API는 Axum이며 candidate를 production-ready 또는 전체 통합 완료로 표시하지 않는다.
