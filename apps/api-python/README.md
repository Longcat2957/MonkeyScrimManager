# FastAPI migration candidate

Docker 배포를 전제로 백엔드를 단계별 이관한다. 현재 구현 단계는 사용자 지시에 따라 **백엔드 테스트와 Docker 검증만으로 완료를 판단**한다. frontend 연결, 5173 proxy smoke, 기존 로컬 Axum reload는 이 단계의 조건이 아니다.

구현 범위의 최신 진행 상황과 잔여 API는 [이관 실행 기록](../../docs/migration/fastapi-progress.md)을 기준으로 한다.

구현 범위:

- `GET/HEAD /healthz`, `GET/HEAD /readyz`, `GET/HEAD /api/me`.
- `GET/HEAD /oauth/discord/start`, `GET/HEAD /oauth/callback`, `POST /api/logout`.
- `GET/HEAD /api/me/guilds`, `GET/HEAD /api/guilds/{guild_id}`, `PUT /api/me/default-guild`.
- 일반 모집 조회·생성·참가·마감·재개·취소·복구, 엔트리 편집, 시리즈·픽밴·경기 결과/MMR.
- Discord 알림 claim/ACK/NACK, heartbeat, 카탈로그, 길드 설정·시즌 관리.
- 경매 모집 목록·상세·생성·참가·취소·마감·재개·복구, 준비 드래프트 조회/저장, 참가 확정 DM 요청.
- 일반 엔트리→10인 경매 모집 전환: 라인 미배정 허용, 기존 시즌·규칙·명단 유지, 배정된 라인 MMR 고정 저장.
- 경매 대회 목록·생성·상세, 추첨·순서 재섞기·유찰·낙찰·수동 배정·유찰 복원·되돌리기·초기화의 입찰 명령 9종.
- 서버 발급 UUIDv7 request ID, 기존 success/error envelope, 보안 header, body limit, timeout.
- SQLAlchemy async/asyncpg pool, SQLx version/success/SHA-384 exact readiness, Valkey degraded 처리.
- 기존 session cookie/hash, session 만료·취소·삭제 사용자 차단, session touch, active default guild, service-admin 표시.
- Discord OAuth state/PKCE, AES-GCM envelope, 1회 callback consume, session rotation, CSRF, 길드 membership/operator 판정.
- Python 3.14.7, uv lockfile, 비특권·read-only Docker 실행, 독립 candidate Compose.

## 설치와 테스트

저장소 루트에서 실행한다.

```bash
uv sync --project apps/api-python --locked
scripts/test-fastapi.sh
```

검증 스크립트는 Docker PostgreSQL을 임의의 loopback port로 생성한다. 현재 SQLx migration 25개와 실제 runtime grant를 적용하고, Python 테스트와 Docker 이미지 build/HTTP smoke를 실행한 뒤 테스트 container/volume을 정리한다. DB credential은 이 임시 환경 전용이며 운영 env 파일을 읽지 않는다.

테스트 단계에는 기존 migration binary와 `fastapi-auth-compat` 검증 도구를 빌드하는 Cargo가 필요하다. 검증 도구는 기존 Rust `AuthService`를 호출해 PKCE 암호문과 session/CSRF hash의 언어 간 호환성을 검사한다. **API Docker runtime에는 Rust/Cargo가 없다.** Alembic 인계는 계획 P7에서 진행한다.

테스트 수와 검증 결과는 이관 실행 기록에 갱신한다. Discord token/user 응답은 HTTPX MockTransport로 대체하며 실제 Discord 로그인이나 메시지 발송을 수행하지 않는다. Docker에서는 HTTP health/readiness, 인증·권한, 일반/경매 모집부터 엔트리·시리즈·경기 결과까지, 알림 ACK와 길드 관리 흐름을 검증한다. Origin/callback URL에 등장하는 `localhost:5173`은 요청 계약용 문자열이며 해당 frontend에 연결하지 않는다.

경매 `RESET_TO_INITIAL`은 기존 Rust처럼 원장과 작업 이력을 삭제한다. 현재 runtime grant가 해당 DELETE를 금지하므로 실제 권한에서는 양쪽 모두 500으로 실패하고 전체 rollback한다. 권한 정책은 변경하지 않았으며, 초기화 알고리즘의 동등성만 격리 테스트에서 임시 DELETE 권한을 부여해 검증한다. Docker에서는 실제 권한의 실패·rollback을 확인한다.

DB 없는 빠른 검증만 필요하면 다음 명령을 사용한다.

```bash
uv run --project apps/api-python --locked pytest apps/api-python/tests/test_foundation.py
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

## 기준 계약과 다음 작업

`docs/migration/fastapi-baseline.json`은 현재 Rust source hash와 명시적 method/path 목록이다. `sqlx-manifest.json`은 API 이미지에 포함되는 migration checksum manifest다.

```bash
python3 scripts/fastapi-baseline.py
# 실제 Rust 계약 변경을 검토하고 새 baseline을 수용할 때만:
python3 scripts/fastapi-baseline.py --write
```

전체 migration은 [세부 계획](../../docs/18-fastapi-migration-plan.md)을 따른다. 다음 항목은 아직 구현·검증되지 않았다.

- 모든 operation의 consumer/auth/DTO/test mapping, 운영 latency/SLO 기준.
- profile/preferences/privacy 등 나머지 P2 API, 일반 내전·경매·관리 API, Bot/Riot 서비스 이관.
- WebSocket handshake와 relay/worker, 전체 graceful-drain deadline 및 다중 instance 동작.
- 기존 SQLx pool의 minimum/idle 정책 등 모든 설정의 동등성, generic body DTO/content-type 계약의 전 route 검증.
- Alembic 인계, 운영 전환·롤백. Web/Bot 연결 전환은 현재 백엔드 구현 범위에서 제외한다.

PKCE는 Rust 발급→Python 복호화와 Python 발급→Rust 복호화를 모두 테스트한다. Rust 발급 OAuth attempt를 DB에 넣고 Python callback에서 완료하는 흐름도 검증한다. 실제 Discord provider와 운영 환경의 로그인 검증은 별도 배포 단계다.

OAuth start rate limit은 burst 30회/분을 유지하며, caller가 임의로 바꿀 수 있는 `X-Real-IP`/`CF-Connecting-IP`를 신뢰하지 않는다. 기존 Rust handler의 해당 동작과 다르지만 [기반 보안 계약](../../docs/decisions/0001-api-foundation.md)의 trusted proxy 미설정 정책을 따른다. 현재는 실제 연결 peer별 제한이므로 reverse proxy 뒤에서는 그 proxy 연결들이 quota를 공유한다. trusted proxy CIDR 설정은 후속 운영 단계에서 다룬다.
