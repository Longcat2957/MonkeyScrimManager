# FastAPI migration candidate

Docker 배포를 전제로 백엔드를 단계별 이관한다. 현재 구현 단계는 사용자 지시에 따라 **백엔드 테스트와 Docker 검증만으로 완료를 판단**한다. frontend 연결, 5173 proxy smoke, 기존 로컬 Axum reload는 이 단계의 조건이 아니다.

Rust 백엔드는 FastAPI 전환 완료 후 대체할 동결 기준이며 더 이상 업데이트하지 않는다. 구현·수정은 FastAPI에 집중하고, 기존 Rust migration/호환성 도구는 Alembic 인계 전까지 수정 없이 테스트 fixture로 사용한다.

동결 목록의 **142/142개 operation(주 API 137개, Riot 서비스 5개)을 구현했다.** 새 경매 마무리와 SQLx 0026도 포함한다. 이는 경로 이관 상태이며 운영 전환 완료를 뜻하지 않는다. 검증 결과는 [이관 실행 기록](../../docs/migration/fastapi-progress.md)을 기준으로 한다.

구현 범위:

- `GET/HEAD /healthz`, `GET/HEAD /readyz`, `GET/HEAD /api/me`.
- `GET/HEAD /oauth/discord/start`, `GET/HEAD /oauth/callback`, `POST /api/logout`.
- `GET/HEAD /api/me/guilds`, `GET/HEAD /api/guilds/{guild_id}`, `PUT /api/me/default-guild`.
- 일반 모집 조회·생성·참가·마감·재개·취소·복구, 엔트리 편집, 시리즈·픽밴·경기 결과/MMR.
- Discord 알림 claim/ACK/NACK, heartbeat, 카탈로그, 길드 설정·시즌 관리.
- 운영자 멤버 목록, lane MMR 수동 조정·멱등 재응답, 멤버 비활성화/복구, 감사 로그 cursor 조회.
- 처리 대기 목록, 활성 시즌 리더보드, 시즌·일반/경매 범위별 통계: 공개 설정과 유효 경기·대회 집계 유지.
- 길드 기록 프로필·전체 기록 검색, 길드 회원/공개 Riot ID 자동완성: 개인정보, 10/20인 기록 묶음, cursor 유지.
- 전체 데이터 내보내기: 운영자·CSRF, 41개 테이블의 일관된 snapshot, 25만 행·챔피언 ID 검증과 감사 로그 유지.
- 전체 데이터 가져오기 preview/apply와 legacy MMR 내보내기·가져오기: archive 검증, 대상 길드 remap, 충돌·만료·재응답 및 원자적 적용.
- Bot 길드 setup·등록, 서비스 관리자 상태·기록 초기화, 환경 flag로 제한한 더미 사용자·그룹·연결·모집 관리 API.
- 사용자 Riot 계정 연결·해제·대표 계정·갱신과 공개 기록 캐시·경기 페이지·갱신 API. 별도 `apps/riot-api-python`에서 내부 Riot API 5개 operation을 제공한다.
- `/ws` 구독·권한 재검사, bounded hub, outbox relay와 만료된 데이터 가져오기 preview 정리 worker.
- 경매 모집 목록·상세·생성·참가·취소·마감·재개·복구, 준비 드래프트 조회/저장, 참가 확정 DM 요청.
- 일반 엔트리→10인 경매 모집 전환: 라인 미배정 허용, 기존 시즌·규칙·명단 유지, 배정된 라인 MMR 고정 저장.
- 경매 대회 목록·생성·상세, 추첨·순서 재섞기·유찰·낙찰·수동 배정·유찰 복원·되돌리기·초기화의 입찰 명령 9종.
- 경매 브래킷 조회, 팀 배치·해제·확정·확정 해제: 10인 결승과 20인 준결승/결승 생성, 입력·결과 없는 경기의 이력 보존 후 재확정.
- 경매 경기 조회·라인업 배정/확정/확정 해제, 세트 결과·MMR·마지막 세트 되돌리기, 준결승 승자 진출, 결승 BO1 조기 종료.
- 경매 대회 결과·취소 미리보기·전체 취소와 Bot 모집 강제 취소: 확정 경기의 MMR 역분개 및 이후 MMR 변경 시 취소 거부.
- 경매 기록 보존 마무리 preview/finish: 확정 경기·MMR·라인업·BO 규칙 보존, 무승자 종료, 종료 후 변경 차단과 결과·Discord 알림·전적 DRAW 유지.
- 서버 발급 UUIDv7 request ID, 기존 success/error envelope, 보안 header, body limit, timeout.
- SQLAlchemy async/asyncpg pool, SQLx exact 이력 또는 Alembic head·provenance readiness, Valkey degraded 처리.
- 기존 session cookie/hash, session 만료·취소·삭제 사용자 차단, session touch, active default guild, service-admin 표시.
- Discord OAuth state/PKCE, AES-GCM envelope, 1회 callback consume, session rotation, CSRF, 길드 membership/operator 판정.
- Python 3.14.7, uv lockfile, 비특권·read-only Docker 실행, 독립 candidate Compose.

## 코드 구조

애플리케이션은 업무 영역별 모듈을 HTTP, 스키마, 서비스, 저장소 계층으로 구분한다.

```text
apps/api-python/
  pyproject.toml
  src/monkeybot_api/
    __main__.py                 # Uvicorn 실행 진입점
    app.py                      # app factory, lifespan, router·오류 처리 등록
    api/
      router.py                 # 도메인 router 조립
      dependencies.py           # SettingsDep, DatabaseDep, DiscordDep
      routers/                  # auth, users, guilds, recruitments, series, auctions, notifications
      authentication.py         # Bot/Browser 인증 선택
      sessions.py               # HTTP cookie와 세션 해석
      body.py, requests.py      # 기존 JSON·header 파싱 계약
      middleware.py
    schemas/                    # 도메인별 Pydantic 입력·출력 및 입력 정규화
    services/                   # 업무 규칙, 권한·revision·멱등성 순서, 변경 작업 조합
    repositories/               # SQL, 행 잠금, 저장·조회, 응답 snapshot
    core/                       # config.py, 오류, 보안, 시각·문자열·cursor 공통 처리
    db/                         # DB pool, transaction/retry, SQLx manifest와 Alembic baseline
    integrations/               # Discord client, Riot cache 계약
    domain/                     # HTTP에 의존하지 않는 도메인 오류·규칙
  tests/
    conftest.py                 # 환경 격리와 공통 HTTP fixture
    unit/                       # DB 없는 계약·보안·구조 테스트
    integration/                # 업무 영역별 PostgreSQL·HTTP 회귀
      conftest.py               # 격리 DB/runtime client fixture
    support/                    # 재사용 fixture builder와 assertion
```

`app.py`는 lifespan에서 공용 자원의 수명을 관리하고 `api/router.py`에서 조립한 router를 등록한다. 각 HTTP 모듈은 `router = APIRouter()`를 내보낸다. Router는 `api/dependencies.py`의 `Depends` 별칭으로 현재 앱의 설정·DB·Discord client를 받는다. 테스트에서는 `app.dependency_overrides`로 자원을 교체할 수 있다.

요청은 **HTTP 파싱·인증 → 서비스의 업무 처리 → 저장소의 SQL** 순서로 흐른다. Router는 기존 content-type/JSON/CSRF 오류와 응답 envelope를 유지하고, 서비스는 권한·상태·revision·멱등성·MMR·알림의 처리 순서를 결정한다. 변경 workflow 전체를 `db.transactions.mutation` 또는 해당 workflow의 명시적 transaction으로 묶고 저장소에 같은 `AsyncConnection`을 전달한다. 모집·시리즈·경매 저장소는 전달받은 connection에서 실행하며 중간 commit하지 않는다. OAuth 저장소는 일회성 attempt 소비와 세션 발급을 독립 transaction으로 유지해 외부 provider 호출 전에 attempt 소비가 확정되도록 한다. `schemas`, `domain`, `repositories`, `services`에서 HTTP router를 역으로 import하지 않는다.

새 기능은 기존 업무 영역의 `schemas`에 계약을 정의하고, `repositories`에 필요한 SQL 연산을 추가한 뒤 `services`에서 조합한다. `api/routers`에는 HTTP 경계와 서비스 호출을 추가하고 새 router 모듈이면 도메인 router 또는 `api/router.py`에 등록한다. 순수 계약은 `tests/unit`, DB·권한·경쟁·rollback은 `tests/integration`에 두며 공통 데이터 준비는 `tests/support`를 재사용한다. URL을 새로 이관한 경우 동결 operation 목록과 대조해 구현 수와 실행 기록을 갱신한다.

## 설치와 테스트

저장소 루트에서 실행한다.

```bash
uv sync --project apps/api-python --locked
scripts/test-fastapi.sh
```

검증 스크립트는 Docker PostgreSQL을 임의의 loopback port로 생성한다. SQLx migration 26개와 실제 runtime grant를 적용하고 Python 테스트를 실행한다. 이어 같은 Docker 이미지의 Alembic CLI로 인계하고 grant를 다시 적용한 뒤 readiness·HTTP smoke를 검사한다. 테스트 container/volume은 정리하며 DB credential은 임시 환경 전용이다.

테스트 단계에는 SQLx 기준 DB fixture와 `fastapi-auth-compat` 검증 도구를 빌드하는 Cargo가 필요하다. 검증 도구는 기존 Rust `AuthService`를 호출해 PKCE 암호문과 session/CSRF hash의 언어 간 호환성을 검사한다. **API Docker runtime과 Alembic CLI에는 Rust/Cargo가 없다.**

테스트 수와 검증 결과는 이관 실행 기록에 갱신한다. Discord token/user 응답은 HTTPX MockTransport로 대체하며 실제 Discord 로그인이나 메시지 발송을 수행하지 않는다. Docker에서는 HTTP health/readiness, 인증·권한, 일반/경매 모집부터 엔트리·시리즈·경기 결과까지, 알림 ACK와 길드 관리 흐름을 검증한다. Origin/callback URL에 등장하는 `localhost:5173`은 요청 계약용 문자열이며 해당 frontend에 연결하지 않는다.

경매 `RESET_TO_INITIAL`은 기존 Rust처럼 원장과 작업 이력을 삭제한다. 현재 runtime grant가 해당 DELETE를 금지하므로 실제 권한에서는 양쪽 모두 500으로 실패하고 전체 rollback한다. 권한 정책은 변경하지 않았으며, 초기화 알고리즘의 동등성만 격리 테스트에서 임시 DELETE 권한을 부여해 검증한다. Docker에서는 실제 권한의 실패·rollback을 확인한다.

DB 없는 빠른 검증만 필요하면 다음 명령을 사용한다.

```bash
uv run --project apps/api-python --locked pytest apps/api-python/tests/unit
```

## Docker 실행

```bash
docker build -f apps/api-python/Dockerfile -t monkeybot-api-python:candidate .
```

별도 인스턴스를 실행할 때는 `deploy/api/fastapi.env.example`을 참고해 **격리된 migration 완료 DB의 runtime credential**을 준비한다. container에서 DB host에 연결할 수 있어야 한다. Docker container의 `127.0.0.1`은 호스트의 DB tunnel이 아니다.

```bash
export MONKEYBOT_FASTAPI_RUNTIME_ENV=/absolute/path/to/candidate-runtime.env
docker compose -f deploy/docker-compose.fastapi.yml up -d --build
curl --fail http://127.0.0.1:30022/readyz
curl -i http://127.0.0.1:30022/api/me
```

`/api/me`는 auth 설정이 있고 쿠키가 없으면 401이다. migration 이력이 불일치하면 readiness는 503이다. Valkey를 생략하면 core DB가 정상일 때 degraded/200이다. healthcheck는 liveness이며 readiness를 대신하지 않는다.

Compose는 main/dev의 service/upstream/port를 바꾸지 않는다. 실제 공개 Docker 배포 전환은 전체 계약 검증 이후다. 현재 백엔드 검증에는 기존 `scripts/local-api-reload.sh`를 실행하지 않는다.

## Alembic 인계 후보

`monkeybot-migrate-python`은 별도의 migration role DB URL을 사용한다. 기존 DB의 `handover`는 SQLx 0001..0026 이력, 고정 schema 지문과 runtime grant를 검사한 뒤 revision `20261003_baseline`과 `sqlx_handover` provenance를 한 transaction에 기록한다. 빈 DB의 `bootstrap`은 같은 business schema를 만들되 `_sqlx_migrations` 이력을 만들지 않고 `alembic_bootstrap` provenance를 기록한다. 두 경로 모두 실행 후 `runtime-grants.sql`을 migration role로 다시 적용해야 한다. API `/readyz`는 인계 전 SQLx 이력과 인계 후 두 provenance를 각각 검사한다.

후보 이미지를 build하고 별도 migration env 파일을 지정한 뒤 현재 이력을 읽기 전용으로 확인할 수 있다.

```bash
export MONKEYBOT_FASTAPI_RUNTIME_ENV=/absolute/path/to/candidate-runtime.env
export MONKEYBOT_FASTAPI_MIGRATION_ENV=/absolute/path/to/candidate-migration.env
docker compose -f deploy/docker-compose.fastapi.yml --profile migration run --rm migrate validate
```

격리 DB에서 backup/restore를 확인하고 모든 API instance를 중지한 전환 창에서만 `validate` 대신 기존 DB는 `handover --runtime-role <runtime-role>`, 빈 DB는 `bootstrap`을 실행한다. 이후 `deploy/postgres/runtime-grants.sql`을 적용하고 `validate --runtime-role <runtime-role>` 및 `/readyz`를 검사한다. 개발/테스트에서 service-admin reset 실행 권한을 의도적으로 켠 경우에만 인계·검증 명령에 `--service-admin-reset-enabled`를 추가한다. 후보 Compose의 migration 작업은 기본값이 `validate`이며 기존 main/dev service를 교체하지 않는다. `/healthz`는 liveness이므로 실제 traffic 전환 gate는 `/readyz`다.

## 기준 계약과 다음 작업

`docs/migration/fastapi-baseline.json`은 현재 Rust source hash와 명시적 method/path 목록이다. `src/monkeybot_api/db/sqlx-manifest.json`은 API 이미지에 포함되는 migration checksum manifest다. 환경 설정은 `src/monkeybot_api/core/config.py`에서 정의한다.

```bash
python3 scripts/fastapi-baseline.py
# 실제 Rust 계약 변경을 검토하고 새 baseline을 수용할 때만:
python3 scripts/fastapi-baseline.py --write
```

전체 migration은 [세부 계획](../../docs/18-fastapi-migration-plan.md)을 따른다. 다음 항목은 아직 구현·검증되지 않았다.

- 모든 operation의 consumer/auth/DTO/test mapping, 운영 latency/SLO 기준.
- Staging의 대표 부하에서 Valkey 장애·다중 instance WebSocket fanout, 전체 graceful-drain deadline 검증.
- 기존 SQLx pool의 minimum/idle 정책 등 모든 설정의 동등성, generic body DTO/content-type 계약의 전 route 검증.
- Alembic의 staging 인계·bootstrap/복원 rehearsal, 이미지·systemd·script 연결 변경, 운영 전환·롤백. Web/Bot 연결 전환은 현재 백엔드 구현 범위에서 제외한다.

PKCE는 Rust 발급→Python 복호화와 Python 발급→Rust 복호화를 모두 테스트한다. Rust 발급 OAuth attempt를 DB에 넣고 Python callback에서 완료하는 흐름도 검증한다. 실제 Discord provider와 운영 환경의 로그인 검증은 별도 배포 단계다.

OAuth start rate limit은 burst 30회/분과 기존 Rust의 `X-Real-IP` → `CF-Connecting-IP` → `unknown` key 선택 순서를 보존한다. trusted proxy 정책 변경은 이번 동등성 이관에 포함하지 않는다.
