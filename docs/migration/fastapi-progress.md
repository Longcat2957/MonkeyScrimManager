# FastAPI migration 실행 기록

기준 계획: [18-fastapi-migration-plan.md](../18-fastapi-migration-plan.md).

## 필수 이관 범위 — 경매 마무리 (2026-10-03)

2026-09-26의 **Axum + Web 선행 구현**에 이어, 2026-10-03 사용자 지시로 새 경매 마무리도 **FastAPI 리팩토링의 필수 범위**로 확정했다. 이전의 FastAPI 구현/baseline 갱신 제외 지침을 대체한다. 두 endpoint와 SQLx 0026, 결과·알림·기록 및 종료 후 변경 차단을 이관했고 아래 스물세 번째 구현 단위에서 검증했다.

세부 acceptance와 소비자 계약은 [계획의 필수 이관 항목](../18-fastapi-migration-plan.md#필수-이관--경매-기록-보존-마무리-2026-10-03)을 따른다. 현재 동결 기준의 **142/142개 operation(주 API 137개, Riot 서비스 5개)**을 구현했고 migration manifest는 26개다. 새 기능을 제외한 상태를 리팩토링 완료로 처리하지 않는다.

## 현재 검증 기준

사용자 지시: frontend 연결을 제외하고 백엔드 구현·테스트로 성공 여부를 확인한다. 이에 따라 현재 단계는 격리 PostgreSQL과 테스트 대역을 통한 계약·권한·경쟁·오류 테스트, Ruff/mypy, Docker build/실행 검증을 완료 조건으로 사용한다. frontend check·build, 5173 proxy smoke, 기존 로컬 Axum reload는 실행하지 않으며 이전 proxy 연결 실패는 현재 blocker가 아니다.

2026-09-20 추가 지시: Rust 백엔드는 전환 완료 시 대체하므로 더 이상 업데이트하지 않는다. 구현과 수정은 FastAPI에 집중한다. 기존 Rust migration/호환성 도구는 Alembic 인계 전까지 수정 없이 fixture로 사용하며, Rust 소스·검증 도구 확장이나 Clippy 정비는 하지 않는다.

## 스물다섯 번째 구현 단위 — Alembic 인계·복원·다중 instance (2026-10-03)

- SQLx 0001..0026의 성공·SHA-384 이력, 고정 catalog 지문과 runtime 권한 정책을 검증한 뒤 한 transaction과 advisory lock 안에서 revision `20261003_baseline`을 stamp하는 `monkeybot-migrate-python handover`를 추가했다. 빈 DB의 `bootstrap`은 같은 business schema를 Alembic으로 생성하고 SQLx 이력을 만들지 않는다. `migration_provenance`는 `sqlx_handover`/`alembic_bootstrap` 출처를 분리하며, 누락·변조·알 수 없는 head·부분 적용은 거부한다.
- `/readyz`는 기존 SQLx-only 상태와 두 Alembic 상태의 정확한 metadata를 확인한다. runtime grant는 실제 존재하는 migration metadata에 SELECT만 허용한다. 일반 서비스 관리자 reset 권한은 기본 거부하며 개발/테스트에서 명시적으로 켠 경우에만 인계 CLI도 허용한다. 제3자 역할의 추가 함수·테이블 권한도 인계 검증에서 거부한다.
- 후보 Compose에 별도 migration 작업을 추가하고 기본 명령을 읽기 전용 `validate`로 두었다. 기존 main/dev 배포를 바꾸지 않은 채 API Docker 이미지 안의 CLI로 인계→grant 재적용→검증→HTTP smoke를 실행한다. 백업·복원 smoke는 SQLx-only, 인계 DB, 신규 Alembic DB의 provenance와 원본/복원본 서명을 대조하고, ACL을 제외한 복원 뒤 target grant를 재적용·검증한다.
- 두 Uvicorn/WS 인스턴스와 일회용 Valkey를 사용해 교차 fanout, Valkey 중단 중 outbox backlog와 복구, lease fencing, 느린 클라이언트의 재조회 신호, 권한 회수를 검증했다. runtime 수정 없이 새 통합 테스트 2개가 통과했다. Staging에서의 Bot notification 소비·대표 부하·graceful drain·양방향 rollback rehearsal과 실제 운영 전환은 아직 수행하지 않았다.
- **최종 백엔드 검증 통과:** `scripts/test-fastapi.sh`에서 격리 PostgreSQL 테스트 **718개**, Ruff, mypy 273개 source, Docker 이미지의 Alembic `handover`→runtime grant 재적용→`validate`와 실제 API HTTP smoke가 통과했다. 별도 백업·복원 smoke도 세 DB 이력 상태를 복원해 검증했고, 누락된 role·변조된 권한·비어 있지 않은 대상을 거부했다. Riot 서비스의 기존 테스트 16개와 Docker smoke 결과는 유지한다. 현재 실행은 후보/격리 DB 전용이며 운영 DB 인계나 공개 traffic 전환은 수행하지 않았다.

## 스물네 번째 구현 단위 — 잔여 경로·Riot·실시간 (2026-10-03)

- 이전 94개에 **주 API 43개**(전체 데이터 가져오기 2, legacy 데이터 전송 3, Bot 길드 setup/등록 2, 서비스 관리자 3, Riot 계정 5·공개 기록 3, 개발 API 24, `/ws` 1)를 추가했다. 별도 `apps/riot-api-python`의 health·내부 Riot API **5개**도 이관해 동결 목록의 **142/142개** method/path가 등록됐다. 개발 API는 `MONKEYBOT_DUMMY_USERS_ENABLED`가 켜진 비운영 환경에서만 등록한다.
- 전체 데이터 가져오기는 41개 테이블 archive의 digest·대상 길드 충돌·시즌/Discord 연결 remap·preview 만료·apply 멱등성과 원자성을 처리한다. legacy 전송은 기존 MMR seed 형식의 export/preview/apply를 유지한다. 두 preview의 만료 정리 worker는 즉시 실행 후 60초 주기로 반복하고, 한 종류의 실패가 다른 종류의 정리를 막지 않는다.
- Riot 계정·기록 경로는 세션/CSRF, 공개 기록 snapshot·최근 경기 page cache·30초 갱신 제한·5분 lease와 안전한 upstream 오류를 보존한다. 독립 Riot 서비스는 내부 Bearer 인증, Riot upstream, 5분 fresh·30분 stale cache, key별 single-flight 및 최근 경기/시즌 집계를 구현했다. 독립 서비스는 fake provider·MockTransport와 Docker health/무인증 smoke로 검증하고 실제 Riot 계정 호출은 하지 않았다.
- `/ws`는 인증·정확한 Origin 거부, 최대 16개 topic의 원자적 구독과 권한 재검사, bounded hub의 `resync_required`, outbox의 `SKIP LOCKED` claim → publish → owner/token 일치 ack/nack을 이관했다. 실제 Uvicorn handshake·fanout·lease 테스트를 추가했고, control Ping/Pong은 Uvicorn의 25초 ping/75초 timeout 설정을 사용한다.
- 최종 고위험 검토에서 Riot 최근 경기 page의 내용 검증 누락을 발견해 수정했다. Serde 최소 예제로 더미 그룹 설명과 사용자 메모·아바타의 생략/명시적 `null`이 Rust에서 모두 동일한 `None`임을 확인하고 Python의 값 보존·멱등 hash도 일치시켰다. 무효 경기는 공개 cache에 저장되지 않는 회귀 테스트를 추가했다. 동결 baseline과 실제 주 API 137개·Riot API 5개 등록을 각각 자동 대조한다.
- **최종 백엔드 검증 통과:** `scripts/test-fastapi.sh`에서 SQLx migration 26개와 runtime grant를 적용한 격리 PostgreSQL 테스트 **714개**, Ruff, mypy 268개 source, Docker build·UID 10001/read-only runtime·실제 HTTP smoke가 통과했다. 별도 Riot 서비스는 테스트 **16개**와 Docker health/무인증 smoke가 통과했다. format, baseline, diff whitespace 검사도 통과했다. Alembic 인계, 다중 instance 부하·drain, Web/Bot 연결 및 운영 전환은 후속 단계다.

## 스물세 번째 구현 단위 — 경매 기록 보존 마무리 (2026-10-03)

- `GET/HEAD /api/guilds/{guildId}/auction-tournaments/{id}/finish-preview`와 `POST .../{id}/finish`의 **2개 operation**을 이관했다. Rust의 신규 두 operation과 SQLx 0026을 포함하도록 동결 기준을 갱신했으며, 실제 앱 등록은 **94/142개**, 기준 외 operation은 없다. 기존 Rust 구현·Web·Bot 소스는 변경하지 않았다.
- preview는 현재 운영자 세션을 요구하고, finish는 typed JSON → 음수 revision/사유 검증 → CSRF → Idempotency-Key 순서를 보존한다. 현재 ACTIVE 길드·회원·운영자 확인을 replay보다 먼저 수행하고, trim한 사유와 Rust 필드 순서로 동일한 request hash를 만든다. `-0` revision의 422, 숫자 범위 초과의 400도 기존 `legacy_json` 파서로 일치시켰다.
- tournament를 먼저 잠가 경기·라인업·브래킷 변경과 직렬화한다. `BIDDING`/`BRACKET_SETUP`/`IN_GAME`만 마무리하며 revision 충돌과 terminal 상태를 구체적 오류로 반환한다. 확정 경기의 DEFAULT/SWAPPED 진영을 반영해 FINAL 스코어 선두만 우승팀으로 정하고 동점·결승 미진행은 null을 유지한다. 미완료 match는 `COMPLETED + ended_early=true`로 끝내고, 이미 완료한 준결승의 결과·종료 시각은 보존하되 모든 match의 revision/invalidate를 갱신한다.
- 확정 경기·game input·MMR 원장과 현재 MMR·라인업·BO 규칙·source 모집의 `CONVERTED` 상태를 보존한다. 감사 로그, match/tournament outbox, 설정된 결과 알림, 멱등 응답을 같은 transaction에 저장한다. 이후 MMR 변경이 있어도 마무리할 수 있으며 outbox 저장 실패 시 상태·감사·멱등 작업이 함께 rollback된다.
- 경기/결과에 `endedEarly`, 경기 권한에 `canFinishTournament`를 반영하고 종료 후 입력·라인업·되돌리기를 차단했다. 무경기·미정 결승 팀·무승자 결과와 nullable Discord 알림을 허용하며 정상 완료의 승수·우승팀 검증은 유지한다. 10인 기록과 20인 내부 시리즈의 승자 null은 `DRAW`로 반환한다.
- 신규 integration **10개**(마무리 8, 기록 검색 2)를 추가했다. 입찰 중 종료, 브래킷 확정 후 무경기 종료, 10인 Bo5 선두·동점, 20인 완료/진행 준결승과 결승 도중 종료, 권한·CSRF·tenant·입력 오류, 중복/경쟁 요청, 실패 rollback과 기록 보존을 검사한다. 기존 lineup/result 기대값도 신규 필드에 맞췄다.
- **`scripts/test-fastapi.sh` 전체 통과**: SQLx migration 26개와 runtime grant, Python **663개 통과(303.93초)**, Ruff, mypy 220개 source, Docker build 및 UID 10001·read-only runtime/HTTP smoke. Docker에서 신규 preview/HEAD/finish/replay, 무승자 결과·실제 notification outbox와 기존 전체 workflow를 검증했다. format/diff 검사와 baseline 검사도 통과했으며 테스트 컨테이너를 정리했다. 프런트엔드 연결·기존 Axum reload·운영 전환은 기존 단계 구분에 따라 수행하지 않았다.

## 스물두 번째 구현 단위 — 전체 데이터 내보내기 (2026-10-03)

- `POST /api/guilds/{guildId}/full-data-export`의 **1개 operation**을 이관했다. 실제 앱 등록 기준으로 **92/140개 구현, 잔여 48개(API 43, Riot 5)**다. 기존 `full-data-imports/preview`와 `apply`, legacy `data-export/data-imports`, 개발 전용 API, 나머지 Bot/admin/Riot/realtime/worker/Alembic/운영 전환은 아직 남아 있다.
- 세션 사용자 인증, CSRF, ACTIVE 길드의 ACTIVE 운영자 권한, `REPEATABLE READ` snapshot, Rust의 `MONKEYBOT_DATA_TRANSFER` / `GUILD_FULL_SNAPSHOT` V1 archive 계약을 보존했다. archive는 회원·시즌·일반/경매 workflow·경기·MMR 원장·감사 이력을 포함하고, 인증 secret·길드 Discord 설정·cache·outbox·idempotency row는 제외한다.
- Rust reference와 같은 41개 table allowlist를 사용한다. 길드 멤버와 dummy profile 생성/수정자를 기준으로 global user/profile row를 포함하고, Riot account는 길드 멤버 사용자만, `user_guild_preferences`는 대상 guild default만, guild-scoped table과 MMR/audit은 대상 guild row만 내보낸다. admin audit archive는 export 요청으로 새로 생성되는 감사 row를 포함하지 않고, 응답 후 DB에는 `FULL_DATA_TRANSFER_EXPORTED` 감사 row를 남긴다.
- 후속 대조에서 빠져 있던 Rust archive 검증을 복원했다. 정확한 41개 table 집합, 전체 250,000행 제한, guild-scoped row와 기본 길드의 source 일치, 다섯 table의 ASCII 영숫자 챔피언 ID(1..64 bytes)를 검사한다. `game_participants`만 명시적 null을 허용하며 누락 필드는 거부한다. Rust와 같은 table 순서·오류 code/message/details를 유지하고, 길드명은 Python 기본 strip 대신 Rust의 Unicode White_Space 규칙으로 검사한다.
- integration test **7개**, unit test **27개**로 export의 데이터 범위·권한·검증을 보호한다. 더미 생성/수정자 global row 포함과 비회원 Riot 계정 제외, 다른 길드 기본 설정 제외, audit checkpoint 제거와 request ID/summary 기록, 비회원·비활성 회원·비활성 길드 숨김, CSRF·운영자 권한, 실패 시 감사 미생성과 transaction rollback, 빈 길드명의 안전한 500을 검사한다. 기존 fixture의 중복 preference, 필수 `is_externally_public` 누락, 존재하지 않는 `recruitments.expected_start_at`과 CSRF 기대값도 수정했다.
- 최초 작업에서 Docker 접근 제한으로 skip됐던 DB 검증을 실제 runtime grant가 적용된 격리 PostgreSQL 18.4에서 완료했다. 내보내기 34개와 architecture 3개, **총 37개 통과**(첫 실행 34개 통과 후 fixture/기대값 수정과 새 오류 test 3개 재검증). Ruff 전체, mypy 216개 source와 format check도 통과했다. Docker 이미지를 build하고 UID 10001·read-only 환경에서 readiness, 기존 전체 HTTP smoke 및 추가 export/archive/CSRF/audit smoke를 통과한 뒤 컨테이너를 정리했다.
- 당시 `scripts/fastapi-baseline.py`는 기존 경매 마무리 변경과 SQLx 0026 때문에 manifest/inventory drift를 보고했다. 이 단계에서는 동결 기준을 덮어쓰지 않고 기존 manifest의 SHA-384를 확인한 SQLx 0001..0025만 임시 DB fixture에 적용해 검증했다. 당시 실제 앱 등록은 동결 inventory 대비 **92/140개**, 추가 operation 없음으로 재확인했다. 이후 경매 마무리·0026 인계와 일괄 `scripts/test-fastapi.sh` 검증은 위 스물세 번째 구현 단위에서 완료했다. Rust 소스, frontend, OpenAPI, 기존 Axum 실행 환경은 이 단계에서 변경하지 않았다.

## 스물한 번째 구현 단위 — 기록 프로필·검색·자동완성 (2026-09-20)

- 길드 기록 프로필 GET, 기록 검색 GET, 길드 회원 자동완성 GET, 공개 Riot ID 자동완성 GET의 **4개 operation**을 이관했다. 각 GET의 HEAD도 지원한다. 실제 앱의 service/method/path를 동결 inventory와 대조해 **91/140개 구현, 잔여 49개(API 44, Riot 5)**를 확인했다. HTTP·스키마·서비스·저장소 경계를 유지하고 회원 식별·공개 identity projection을 프로필과 기록 검색에서 공유한다.
- 프로필은 ACTIVE 길드의 ACTIVE 요청자·대상만 허용한다. canonical Discord/더미 ID 우선 exact match, 길드 표시명 exact match, 공개된 Riot ID 정규화 match와 중복 대상 409를 보존한다. 선호 챔피언 순서, main 우선 계정 목록, 공개 Discord 사용자만 가지는 profileHref, 안전한 avatar URL을 반환한다. ID나 표시명 조회는 비공개 Riot 연결을 노출하지 않으며, Riot ID로 대상을 찾은 뒤 공개 설정이 회수되면 다시 404로 숨긴다.
- 기록 검색은 READ ONLY REPEATABLE READ에서 동일한 회원 식별과 현재/지정 시즌을 읽는다. 다섯 lane의 기본 MMR, 경기·챔피언 집계, 완료 일반 시리즈와 10인 경매 결승의 SERIES row, 20인 대회의 TOURNAMENT row 및 내부 준결승·결승을 보존한다. 확정 경기·양 팀 명단·진영·밴·대상 MMR before/after를 포함하고 취소/삭제 부모·VOIDED 경기를 제외한다. MMR 원장이 없는 legacy 기록은 delta를 null로 유지한다.
- Outer 기록의 `(playedAt DESC, id DESC, kind DESC)` 순서, nanosecond canonical cursor, SQLx의 PostgreSQL timestamp 절단, limit 1..50과 limit+1 조회를 유지한다. 시즌이 없거나 기록이 없는 대상은 기본값과 빈 목록을 반환하고, foreign/없는 시즌은 같은 404를 반환한다. 세션을 query보다 먼저 추출하며 잘못된 입력의 기존 code·message·request ID를 보존한다.
- 자동완성은 최소 두 글자·최대 8개, literal prefix 검색과 기존 정렬을 유지한다. 길드 회원 후보는 공개된 main Riot 연결만 덧붙이고, 공개 Riot 후보는 로컬 계정·기록 캐시에서 중복/최신성을 처리하며 사용자 연결 정보와 PUUID를 반환하지 않는다. 외부 Riot 호출 없이 조회한다. 동결 Rust의 Unicode NFKC·casefold 버전과 White_Space 규칙도 `core/riot_identity.py`에 보존한다.
- **신규 74개 테스트**(프로필 14, 기록 검색 19, 자동완성 25, 문자 정규화 16)를 추가해 전체 수집 범위는 **619개**다. 이번 변경의 단위·신규 조회·기존 개인정보 영향 범위 **172개를 검증**했다. DB fixture의 날짜 바인딩, bigint/smallint 공용 파라미터와 DUMMY identity 참조 오류를 수정하고 해당 실패를 재검증했다. 전체 기존 suite는 반복하지 않았다. 동결 Rust 정규화와 문자열 **5,000건** 비교도 일치했다. 누락/미지원 query와 매우 긴 limit의 400 응답, 길드 자동완성의 인증 우선 순서를 최종 검토에서 보완했다.
- Ruff, mypy 212개 소스, format, 동결 baseline, 문서 포맷, diff whitespace 검사 통과. 최종 Docker build와 비특권·read-only 실행, 기존 전체 HTTP smoke 및 신규 4개 조회의 privacy·정규화·모호한 이름·경기 상세·취소 반영·종료 시즌·HEAD·오류 검증이 통과했고 격리 컨테이너를 정리했다. Rust 소스, frontend와 기존 Axum 실행 환경은 변경하지 않았다.
- 다음 범위는 데이터 가져오기/내보내기, 나머지 Bot·관리·개발 전용 API와 Riot 조회·계정 관리·서비스다. WebSocket/worker·Alembic·운영 전환도 남아 있다.

## 스무 번째 구현 단위 — 처리 대기·리더보드·시즌 통계 (2026-09-20)

- 처리 대기 목록, 리더보드, 시즌 통계 GET/HEAD의 **3개 operation**을 이관했다. 실제 앱의 HEAD 제외 service/method/path를 동결 inventory와 대조해 **87/140개 구현, 잔여 53개(API 48, Riot 5)**를 확인했다. 각 기능은 `api/routers/guilds`·`schemas/guilds`·`services/guilds`·`repositories/guilds`로 분리한다.
- 처리 대기 목록은 일반 모집 편성, 시리즈 픽밴/결과, 경매 팀장 설정·입찰·대진·결과 입력을 기존 상태·문구·href로 반환한다. ACTIVE guild/member 접근, JOINED 인원/배정 수, CONFIRMED 세트 수, 전환/취소/삭제 제외, 우선순위→갱신 시각→종류→UUID 순서와 최대 100개를 보존한다. 경매의 다음 입력 경기는 진행 상태·준결승 순서로 선택한다.
- 리더보드는 활성 시즌의 ACTIVE 회원과 실제 플레이한 lane만 노출한다. Riot ID 공개 설정, 본인 표시, 가중 MMR, 주력 lane과 대표 챔피언 동률 순서를 유지한다. Python의 보정 합산을 사용하지 않고 Rust의 순차 f64 합산과 `-0.0` 초기값을 보존해 부호가 다른 0의 순위도 일치시켰다.
- 통계는 현재/지정 시즌과 ALL·REGULAR·AUCTION 범위의 경기·참가·진영 승리·데이터 보유 수, 활동/승률 순위, 듀오·챔피언·밴, 완료 대회·팀장·최종 유상 영입 명단을 집계한다. 취소/삭제 부모와 VOIDED 경기를 제외하고, 최소 3경기 승률·상위 10개·정렬 규칙을 보존한다. 시즌 UUID는 Rust가 지원하는 네 가지 표현만 허용하고 query shape→인증→값 검증 순서와 오류를 유지한다.
- 리더보드와 통계는 각각 READ ONLY REPEATABLE READ transaction으로 권한·시즌·집계를 일관된 snapshot에서 읽는다. 다른 길드·비활성 회원·비활성 길드를 숨기고, 저장 데이터 불변식 오류는 안전한 500과 request ID로 응답하며 진단은 서버 로그에 남긴다.
- **신규 46개 테스트**(처리 대기 12, 리더보드 23, 통계 11)를 추가했다. 전체 545개 실행에서 543개가 통과했고(275.98초), 길드 조건·라인업 잠금이 누락된 두 테스트 fixture를 보정한 후 실패했던 두 건을 재실행해 모두 통과했다. 애플리케이션 코드는 이 재검증에서 변경하지 않았다. Ruff, mypy 197개 소스, format, 동결 baseline, 문서 포맷, diff whitespace 검사 통과. 최종 Docker 이미지에서 기존 전체 흐름과 신규 3개 조회의 상태·href·공개 설정·범위별 집계·되돌리기 반영·HEAD·오류를 실제 HTTP로 검증했고 테스트 컨테이너를 정리했다. Rust 소스, frontend 연결과 기존 Axum 실행 환경은 변경하지 않았다.
- 다음 범위는 기록·검색·데이터 내보내기/가져오기와 나머지 Bot·개발 전용 API다. Riot·WebSocket/worker·Alembic·운영 전환도 남아 있다.

## 열아홉 번째 구현 단위 — 운영자 멤버 관리·MMR 조정·감사 로그 (2026-09-20)

- 멤버 GET, lane MMR PATCH, 멤버 상태 PATCH, 감사 로그 GET의 **4개 operation**을 이관했다. 실제 앱의 HEAD 제외 method/path를 동결 inventory와 대조했으며 **84/140개 구현, 잔여 56개(API 51, Riot 5)**다. 기존 `api/routers`·`schemas`·`services`·`repositories` 경계를 유지하고 라우터와 서비스에 SQL을 넣지 않았다.
- 멤버 목록은 운영자 전용 projection으로 Discord/더미 identity, 현재 역할, main Riot ID, 등록·활성 상태와 다섯 lane MMR을 반환한다. 공개 privacy 설정과 무관하게 운영자가 main Riot ID를 확인하는 계약, ACTIVE 우선·이름·ID 정렬, 미생성 lane의 1500 기본값을 유지한다. MEMBERS state나 활성 시즌이 없어도 GET은 데이터를 생성하지 않는다.
- MMR 조정은 현재 운영자 확인 → 멱등성 acquire/replay → MEMBERS revision → 대상 멤버 → 활성 시즌 → lane MMR 잠금 순서를 보존한다. `MANUAL_ADJUSTMENT` 원장, MMR, MEMBERS revision, 감사 로그, 저장 응답을 한 transaction으로 commit하며 경기 수·승수는 유지한다. 음수 조정과 INACTIVE 대상도 기존처럼 허용한다. 원문 note는 hash에 포함하고 원장·감사에는 trim한 사유를 저장한다.
- 상태 변경은 별도 idempotency 없이 CSRF·revision으로 직렬화한다. 자기 자신 INACTIVE는 거부하고, 같은 상태를 다시 지정해도 revision·audit을 남긴다. 비활성화 시각을 기록/해제하면서 등록 정보·기존 MMR·원장은 유지한다. 두 변경 모두 WebSocket invalidate나 Discord 알림을 새로 만들지 않는다.
- 감사 로그는 ACTIVE 운영자에게 현재 표시명과 원본 summary JSON을 반환한다. tenant 격리, 알 수 없는 action과 표시명 누락 보존, `(created_at DESC, id DESC)` keyset, canonical `createdAtNanos` cursor, 1..100 limit을 유지한다. SQLx의 PostgreSQL epoch 기준 submicrosecond 절단도 양쪽 경계에서 검증했다.
- 최종 검토에서 Python과 동결 Rust의 소수 파싱 차이가 기존 MMR 요청 replay hash를 바꾸는 문제를 발견했다. 멤버 변경 요청에만 `core/legacy_json.py`의 숫자 파싱을 적용하고, Pydantic JSON 직렬화로 Rust의 필드 순서·f64 표기를 보존한다. 동결 serde_json 파서와 소수·큰 정수·지수 경계 **2,023건**을 비교했고, 기존 Rust hash를 넣은 완료 요청의 재응답과 원장 미생성을 검증했다. 다른 route의 기본 JSON 파싱은 유지한다.
- **신규 65개 테스트**(멤버 35, 감사 로그 6, 숫자 호환성 24)를 추가해 전체 수집 범위는 **499개**다. 먼저 전체 469개가 통과했고(249.57초), 숫자 파싱·진단 로그 보정 이후 단위/길드 **158개를 재실행해 모두 통과**했다(25.58초). Ruff, mypy 185개 소스, format, 동결 baseline, 문서 포맷, diff whitespace 검사 통과. 최종 Docker 이미지의 health/readiness, 멤버 조회→MMR 조정/재응답→비활성화/복구→감사 페이지 및 기존 전체 HTTP smoke가 통과했으며 테스트 컨테이너를 정리했다.
- 다음 범위는 P5의 기록·통계·검색·리더보드, 데이터 내보내기/가져오기와 나머지 Bot·개발 전용 API다. Riot·WebSocket/worker·Alembic·운영 전환도 남아 있다. Rust 소스 수정, 실제 Discord 발송, frontend 연결과 운영 배포는 수행하지 않았다.

## 구조 리팩토링 — FastAPI 계층·도메인·테스트 구성 (2026-09-20)

- 단일 패키지에 나열했던 모듈을 `api/routers`, `schemas`, `services`, `repositories`, `core`, `db`, `integrations`, `domain`으로 재구성했다. 각 계층 안에서 일반 모집·시리즈·경매·길드·알림·인증 영역을 구분한다. 실제 트리와 기능 추가 절차는 [API README](../../apps/api-python/README.md#코드-구조)와 [계획 4절](../18-fastapi-migration-plan.md#4-적용-기술과-코드-구조)에 기록한다.
- `app.py`는 app factory/lifespan/router 조립을 담당하고, router는 모듈 수준 `APIRouter`를 내보낸다. handler는 `api/dependencies.py`의 `SettingsDep`, `DatabaseDep`, `DiscordDep`를 통해 현재 앱의 자원을 받는다. 앱마다 dependency override가 분리되도록 구성한다.
- HTTP 파싱·인증·CSRF와 응답 처리는 `api`, Pydantic 모델과 입력 정규화는 `schemas`, 업무 규칙·권한·revision·멱등성 순서는 `services`, SQL·행 잠금·snapshot은 `repositories`에 둔다. 서비스 workflow의 여러 저장 작업은 같은 connection/transaction을 공유하고 공통 deadlock 재시도는 `db/transactions.py`로 모았다. 하위 계층에서 HTTP 계층으로 향하던 import도 제거했다.
- 설정은 `core/config.py`, SQLx checksum manifest는 `db/sqlx-manifest.json`, 외부 Discord/Riot 코드는 `integrations`로 이동했다. 이전 flat 경로를 위한 재노출 모듈을 남기지 않고 실제 소비자의 import를 새 위치로 변경한다.
- 테스트는 `tests/unit`, `tests/integration/<업무 영역>`, `tests/support`로 나눴다. 공통 HTTP fixture와 PostgreSQL fixture의 `conftest.py`도 분리했다. 새 구조 검사는 하위 계층의 HTTP 역참조, router factory 재도입, 앱 간 dependency 혼용을 방지하는 3개 시나리오다.
- 이번 변경은 API를 새로 이관하거나 정책을 변경하지 않는다. **구현 80/140개, 잔여 60개(API 55, Riot 5)**를 유지하며 동결된 요청·응답·권한/재응답 순서·transaction 계약을 보존한다. Rust 변경, frontend 연결, 운영 전환은 수행하지 않는다.
- **구조 변경 후 전체 백엔드 테스트 434개 통과**(기존 431개 + 구조·의존성 검증 3개, 253.50초). Ruff, mypy 176개 소스, format, 동결 baseline, 문서 포맷, diff whitespace 검사 통과. 실제 앱의 HEAD 제외 80개 method/path가 동결 inventory와 일치함을 확인했다. Docker build와 health/readiness부터 인증·길드·일반/경매 모집·브래킷·라인업·경기/MMR·결과·취소·알림의 실제 HTTP 흐름까지 통과했으며 임시 컨테이너를 정리했다. 아래 431개 통과 기록은 구조 변경 이전 결과다.

## 열여덟 번째 구현 단위 — 경매 라인업·경기/MMR·결과·취소 (2026-09-20)

- 경기 GET, 라인업 PATCH/lock/unlock, 경기 결과 POST/마지막 세트 DELETE/결승 BO1 조기 종료 POST, 대회 결과 GET, 취소 미리보기 GET/취소 POST, Bot 모집 강제 취소 POST의 **11개 operation**을 이관했다. 실제 등록 method/path를 동결 inventory와 대조했으며 **80/140개 구현, 60개 남음(API 55, Riot 5)**이다. 자동 HEAD와 개별 workflow 동작을 별도 API로 중복 집계하지 않는다.
- 라인업은 양 팀의 현재 roster와 다섯 lane, 선수 중복·팀 소속을 검사한다. 확정 시 대회를 IN_GAME으로 전환하며 20인은 양쪽 준결승 라인업이 모두 확정되어야 결과를 입력할 수 있다. 확정 해제는 경기 입력·결과 이력이 전혀 없는 READY 경기에서만 허용한다. 확정 알림은 그 시점의 명단·MMR snapshot을 보존한다.
- 경기 결과는 DEFAULT/SWAPPED 진영, 세트 순서, 픽·밴 중복과 경기별 하드 피어리스, lane별 Elo/원장을 보존한다. 준결승 승자와 라인업을 결승에 전달하고, 결승의 후속 편성·경기 작업이 있으면 준결승 되돌리기를 거부한다. BO3/BO5 결승의 첫 세트 1:0에서 BO1 조기 종료 시 원본 모집·대회·경기 규칙과 알림을 함께 갱신한다.
- 마지막 세트 되돌리기는 원장 이력을 삭제하지 않고 GAME_REVERSAL을 추가한다. 전송 중인 결과 메시지는 되돌리기를 차단하고, 미전송·전송 완료·전송 불확실 상태는 기존 Rust의 실패 처리·삭제 알림·거부 규칙을 따른다. 대회 결과는 확정 경기 점수·참가자 MMR 증감과 우승 팀을 조회하며, 취소 보관함에서는 브래킷 재확정으로 남은 과거 경기까지 포함한다.
- 대회 취소와 Bot 모집 강제 취소는 최신 확정 경기부터 MMR을 역분개하고 경기·대회·원본 모집을 함께 취소한다. 이후 미취소 경기나 DATA_IMPORT_SEED 등 MMR 변경이 있으면 전체 rollback한다. Browser 취소도 원장·게임 취소의 actor_type=BOT을 기록하는 기존 Rust 계약을 보존한다. Browser 권한 검사는 재응답보다 먼저, Bot actor 동기화·권한 검사는 재응답 이후 수행한다.
- 요청 hash의 중첩 ID/필드 순서와 UUID 정규화, revision, 안전한 공개 오류·request ID, audit·notification·invalidate·idempotency의 transaction 경계를 유지한다. 독립 고위험 검토에서 취소 권한·MMR 이력 안전성·원자성의 구체적 Rust 동작 차이는 발견하지 못했다.
- 신규 테스트는 **라인업 11개, 경기 결과 15개, 취소 9개, 결과 조회 6개로 총 41개**다. 10/20인 흐름, 재응답·동시 요청·권한 회수, 결과 메시지 상태, import checkpoint 거부, MMR 처리 이후 감사/outbox 실패와 다중 경기 취소 중간 실패의 전체 rollback을 포함한다.
- **전체 백엔드 테스트 431개 통과(225.61초).** Ruff/mypy/format/baseline 및 diff whitespace 검사 통과. Docker build와 실제 HTTP 라인업 배정→확정/해제→세트 기록→결승 BO1 조기 종료→결과 조회→마지막 세트 되돌리기→대회 취소/보관함, Bot 모집 강제 취소 및 모든 변경의 재응답까지 통과했다. 신규 MMR 취소 테스트에서 발견한 Bot 역분개 SQL 바인딩 타입 오류를 수정한 뒤 전체 회귀를 통과했다. 격리 테스트 컨테이너는 모두 정리했다.
- 다음 구현 범위는 P5의 회원 관리·MMR 조정·감사·기록/통계·데이터 가져오기/내보내기와 나머지 Bot/개발 전용 API다. Riot·WebSocket/worker·Alembic·운영 전환도 남아 있으며 전체 migration 완료를 뜻하지 않는다. 실제 배포·Discord 발송·프런트엔드 연결은 수행하지 않았다.

## 열일곱 번째 구현 단위 — 경매 브래킷 조회·편집·확정·복구 (2026-09-20)

- 브래킷 GET과 명령 POST의 **2개 operation**, 명령 내부의 **ASSIGN_TEAM, CLEAR_SLOT, LOCK_BRACKET, UNLOCK_BRACKET 4종**을 이관했다. 실제 method/path를 동결 inventory와 대조했으며 **69/140개 구현, 71개 남음(API 66, Riot 5)**이다. 자동 HEAD와 명령 종류는 별도 API로 집계하지 않는다.
- 운영자가 팀을 다른 슬롯에 배치하면 해당 팀의 기존 슬롯과 대상 슬롯을 비운 뒤 배치한다. 빈 슬롯 해제도 revision을 증가시킨다. 10인은 A_0/A_1만, 20인은 네 슬롯 모두 사용하며 확정 시 모든 팀의 활성 roster 5명과 중복 없는 전체 배치를 검사한다.
- 10인은 결승 1개, 20인은 준결승 2개와 팀 미정 결승 1개를 LINEUP_PENDING으로 생성한다. 기존 BO·hardFearless를 보존하며 확정 직후 대회 상태는 BRACKET_SETUP이다. 확정 해제는 활성 경기에 입력·결과 이력이 하나라도 있으면 거부하고, 허용될 때 기존 경기와 lineup을 삭제하지 않고 CANCELLED 이력으로 남긴다. 재확정은 새 경기를 생성한다.
- 조회는 완료·취소 이력, 고정 네 슬롯, 팀별 roster, 확정 결과만 반영한 DEFAULT/SWAPPED 승점, 현재 운영자 capability를 반환한다. 입찰 중 조회는 409이고 타 길드·미가입자의 존재 정보는 404로 숨긴다. canManageMatches는 기존 계약처럼 false다.
- 현재 운영자 권한을 idempotency replay보다 먼저 검사하고, revision/잠금 순서와 요청 hash의 필드 순서·UUID 정규화를 보존한다. 경기·슬롯·대회 변경, 감사 로그, invalidate event, idempotency 응답을 같은 transaction으로 저장한다. Discord 알림·MMR·포인트를 변경하지 않는다.
- Rust 소스나 검증 도구를 확장하지 않았다. 동결 구현과 계약을 대조하고 Python PostgreSQL 테스트로 10인/20인 흐름, 이력 보존·재확정, 경쟁·재응답, 권한 회수, 입력 오류, 점수·종료 상태 조회, audit/outbox 실패 rollback을 검증한다.
- **신규 24개를 포함한 전체 백엔드 테스트 390개 통과.** Ruff/mypy/format/baseline 및 diff whitespace 검사 통과. Docker build와 실제 HTTP 브래킷 조회→팀 배치/해제→확정→확정 해제→재확정, 각 명령의 동일 요청 재응답 검증까지 통과했다. 테스트 컨테이너는 종료·정리했다.
- 다음 경매 범위는 라인업·경기 결과·대회 종료/취소와 Bot 강제 취소다. 실제 배포·Discord 발송·프런트엔드 연결은 수행하지 않았다.

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
