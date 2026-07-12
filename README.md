# MonkeyBot

SvelteKit frontend와 독립 API 서비스로 새로 작성하는 multi-guild 내전 운영 시스템.

## 먼저 볼 것

```txt
1. IMPLEMENTATION_ROADMAP.md
2. docs/09-backend-development-plan.md
3. docs/01-purpose.md
4. docs/06-expected-workflows.md
5. docs/05-db-schema.md
```

## 현재 결정된 것

```txt
개발 순서: Frontend contract -> Backend -> Bot integration
Frontend: SvelteKit frontend only
API: Rust + Axum 별도 backend service
인증: Discord OAuth2 only
디자인: Tailwind CSS + Bits UI + lucide-svelte
배포: VPS
DB: VPS 내부 PostgreSQL
실시간 전달/cache: Valkey
GitHub: repo/history 관리
CI/CD: 기본 제외, 로컬에서 Docker build/push/deploy
범위: 여러 Discord guild를 독립 운영하는 구조
금지: shadcn-svelte, Skeleton UI, daisyUI, 별도 회원가입
```

## 핵심 운영 흐름

```txt
bot 설치
  -> /초기설정
  -> /g/{guildId} dashboard
  -> 모집 / 편성 / 픽밴 / 결과 / 경매 / 전적 검색
```

## 핵심 원칙

```txt
guild_id가 모든 운영 데이터의 경계
Discord OAuth2가 유일한 웹 인증
일반 내전은 MMR 반영
경매 내전도 MMR 반영
경매 match는 게임 시작 전 5라인업 확정 필수
중요 작업은 operation_log 기반 되돌리기 지원
```

## 문서 지도

```txt
IMPLEMENTATION_ROADMAP.md       실제 구현 순서
docs/01-purpose.md              프로젝트 목적
docs/02-roadmap.md              제품/기능 로드맵
docs/03-design-direction.md     디자인 방향
docs/04-routing-plan.md         라우팅 문서 색인
docs/05-db-schema.md            DB 구조와 데이터 흐름
docs/06-expected-workflows.md   예상 사용 workflow
docs/07-realtime-collaboration.md 동시 편집과 WebSocket 계약
docs/08-component-entrypoints.md UI 컴포넌트 진입점과 호출 계약
docs/09-backend-development-plan.md Axum/PostgreSQL 상세 실행계획
docs/routing/frontend.md        Frontend route
docs/routing/backend.md         Backend API route
docs/routing/bot.md             Bot command/component route
```

## 작업할 때 기준

```txt
세부 구현 전에는 IMPLEMENTATION_ROADMAP.md부터 수정
Backend 계약/단계 변경은 docs/09-backend-development-plan.md에 반영
구조/흐름 변경은 docs/06-expected-workflows.md 또는 docs/05-db-schema.md에 반영
라우팅 변경은 docs/routing/*에 반영
디자인 결정은 docs/03-design-direction.md에 반영
컴포넌트 추가/이동/계약 변경은 docs/08-component-entrypoints.md에 반영
```

## 개발 서버 규칙

```txt
dev 환경은 다른 shell에서 계속 실행/확인한다.
Codex는 pnpm dev / pnpm run dev / Vite dev server를 직접 실행하지 않는다.
Codex 검증은 pnpm check / lint / test / build 중심으로 수행한다.
```

## Backend 검증

Arch Linux에서 VS Code `rust-analyzer`를 사용하려면 먼저 `sudo pacman -S --needed rust-src`를
실행하고 language server를 재시작한다.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.dev.yml config
```

실제 PostgreSQL 통합 테스트는 Docker로 PostgreSQL 18.4를 자동 실행한다. 복구 smoke는 권한을 제한한
libpq service/pass file과 서로 다른 `MONKEYBOT_BACKUP_SOURCE_SERVICE`,
`MONKEYBOT_RESTORE_TARGET_SERVICE`를 지정해 `scripts/postgres-backup-restore-smoke.sh`로 실행한다. 이
script는 exported repeatable-read snapshot으로 dump하고 DSN을 child process argument에 넣지 않는다.

## VPS PostgreSQL 개발 접속

개발 데이터베이스는 VPS의 `monkeybot-postgres` 하나를 기준으로 사용한다. PostgreSQL은 VPS
loopback `127.0.0.1:10010`에만 바인딩하며 인터넷에 직접 공개하지 않는다. 로컬 API와 migration은
SSH 터널의 별도 포트 `127.0.0.1:15432`로 접속한다.

```bash
scripts/vps-db-tunnel.sh
```

Host에서 migration이나 API를 실행할 때는 ignored `.env.local`을 명시적으로 source한다. Axum은
env 파일을 자동으로 읽지 않는다. `.env.local`의 database URL은 모두 `127.0.0.1:15432`를
가리켜야 한다.

```bash
set -a
. ./.env.local
set +a

cargo run --locked -p monkeybot-api --bin monkeybot-migrate
cargo run --locked -p monkeybot-api --bin monkeybot-api
```

현재 API는 health/readiness 외에 Discord OAuth/session, `/api/me`, 접근 가능 guild 목록,
대표 guild 설정, guild layout context endpoint를 제공한다. `/readyz`는 PostgreSQL 연결과 embedded
migration의 version/success/checksum 일치를 확인한다.
