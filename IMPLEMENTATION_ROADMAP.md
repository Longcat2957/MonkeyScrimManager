# MonkeyScrimManager Implementation Roadmap

작성일: 2026-07-10

## 목적

이 문서는 실제 구현 순서를 잡기 위한 작업 문서다.

현재는 거의 빈 문서로 유지한다. 세부 구현 계획은 협의하면서 채운다.

## 개발 전제

```txt
Primary app: SvelteKit fullstack
Development order: Frontend -> Bot -> Backend
Design system: Tailwind CSS + Bits UI + lucide-svelte
Auth: Discord OAuth2 only
Scope: multi-guild
Deploy target: VPS
Database: Cloudflare D1
```

주의:

- SvelteKit을 중심으로 먼저 화면과 사용 흐름을 잡는다.
- Frontend는 mocking data 기반으로 먼저 완성한다.
- 이후 필요하면 fake API로 바꾸고, 마지막에 실제 API로 전환한다.
- Frontend의 모든 계획 페이지가 구현 완료되기 전에는 Bot Phase로 넘어가지 않는다.
- Bot은 화면 흐름과 딥링크가 정리된 뒤 붙인다.
- Backend/Core는 화면과 Bot이 요구하는 계약을 기준으로 확정한다.
- Backend 구현은 Frontend 전체 페이지가 실제 운영 가능한 수준으로 완성된 뒤 시작한다.
- VPS 배포를 기준으로 SvelteKit은 Node server 형태로 빌드한다.
- D1은 Cloudflare Worker binding이 아니라 remote DB/API 접근 계층으로 다룬다.
- 한국인 사용자를 대상으로 하므로 사용자에게 보이는 UI 문구는 한국어를 기본으로 한다.
- 사용자 화면에서는 딱딱한 기계적 용어와 내부 구현 용어를 피하고, 자연스럽고 짧은 한국어 표현을 우선한다.
- dev 환경은 다른 shell에서 계속 실행하고 확인한다. Codex는 사용자가 명시적으로 지시하지 않는 한 `pnpm dev`, `pnpm run dev`, Vite/SvelteKit dev server를 직접 실행하지 않는다.
- Codex의 기본 검증은 `pnpm check`, `pnpm lint`, `pnpm test`, `pnpm build`로 수행한다.
- 이 문서는 상세 스프린트 계획이 아니라 구현 순서의 뼈대다.

## Runtime / Deploy 결정

```txt
Deploy: VPS
Web runtime: Node.js
SvelteKit adapter: @sveltejs/adapter-node
Process manager: 추후 결정
Reverse proxy: 추후 결정
DB: Cloudflare D1
DB access: D1 remote API/client layer
Deployment reference: ../mookbot_v3
GitHub: repository / history only
CI/CD: 기본적으로 붙이지 않음
```

원칙:

- Cloudflare Workers 배포를 전제로 하지 않는다.
- Web, Bot, Backend는 VPS에서 돌아가는 Node 프로세스를 기준으로 생각한다.
- SvelteKit fullstack을 우선하고, 별도 Backend 분리는 필요해질 때 확정한다.
- D1 접근 코드는 나중에 Postgres/SQLite 등으로 바꿀 수 있게 얇은 repository 경계 뒤에 둔다.
- 배포 방식은 기존 `../mookbot_v3`의 VPS 배포 흐름을 참고한다.
- GitHub는 repo 유지와 이력 관리 용도로 사용한다.
- Docker image build/push와 VPS 배포는 로컬에서 실행한다.
- GitHub Actions 기반 CI/CD는 기본 범위에 포함하지 않는다.

기존 v3에서 계승할 배포 흐름:

```txt
1. working tree clean 확인
2. main branch / origin main sync 확인
3. D1 schema migrate
4. Docker image build
5. Docker image push
6. VPS Docker disk cleanup
7. VPS docker compose pull
8. VPS docker compose up -d
9. post-deploy cleanup / compose ps 확인
10. Discord slash command 등록
```

배포 실행 위치:

```txt
local machine
  -> preflight
  -> D1 migrate
  -> Docker build
  -> Docker push
  -> ssh VPS
  -> docker compose pull/up
  -> slash command deploy
```

GitHub 사용 범위:

```txt
사용
  source repository
  branch / commit history
  release 기준 tag 선택 가능

비사용
  GitHub Actions build
  GitHub Actions deploy
  registry push 자동화
  VPS 자동 배포
```

참고 파일:

```txt
../mookbot_v3/docs/v3-deployment-plan.md
../mookbot_v3/scripts/deploy-vps.sh
../mookbot_v3/scripts/docker-build.sh
../mookbot_v3/scripts/docker-push.sh
../mookbot_v3/apps/web/Dockerfile
../mookbot_v3/apps/bot/Dockerfile
../mookbot_v3/apps/api/Dockerfile
```

새 프로젝트 배포 초안:

```txt
Docker images
  monkeyscrim-web
  monkeyscrim-bot

VPS services
  nginx 또는 caddy
  web
  bot
  optional valkey
```

주의:

- SvelteKit fullstack을 우선하므로 `api` 이미지를 따로 둘지는 Backend Phase에서 확정한다.
- Backend를 SvelteKit server route로 충분히 처리하면 `web + bot` 2개 앱 이미지로 시작한다.
- 별도 API 프로세스가 필요해지면 기존 v3처럼 `web + api + bot` 3개 이미지로 분리한다.
- slash command 등록은 새 bot 컨테이너가 올라온 뒤 실행한다.
- D1 migrate는 Docker image build 전에 실행한다.

## Framework 결정 현황

확정:

```txt
SvelteKit
@sveltejs/adapter-node
Tailwind CSS
Bits UI
lucide-svelte
Drizzle ORM
zod
Cloudflare D1
Discord OAuth2 only
discord.js
```

추가 결정 필요:

```txt
Validation / contracts
  - contracts를 별도 package로 둘지 여부

Process / deploy
  - Docker Compose
  - systemd
  - PM2
  - GitHub Actions 사용 안 함

Reverse proxy
  - nginx
  - Caddy
  - Traefik

Cache / draft
  - D1만으로 시작
  - Redis/Valkey 추가
  - Cloudflare KV remote 사용

Frontend utilities
  - TanStack Table 도입 여부
  - form helper 도입 여부
  - chart library 도입 여부

Testing
  - Vitest
  - Playwright
```

현재 기본안:

```txt
DB query layer: Drizzle
Validation/contracts: zod 우선
Process/deploy: 기존 mookbot_v3처럼 로컬 Docker build/push + VPS Docker Compose 우선
Reverse proxy: 기존 mookbot_v3 nginx 구성 참고, Caddy는 대안
Cache/draft: D1만으로 시작, 필요 시 Redis/Valkey
Frontend state: Svelte 기본 기능 우선
Testing: Vitest + Playwright
```

## Phase 1: Frontend

현재 수정된 우선순위:

```txt
1. 공통 product shell / navigation hierarchy / layout primitive
2. guild context, operator state, command/search, operations status rail
3. route frame과 mock data 계약
4. route별 실제 작업 화면
```

주의:

- page-by-page placeholder를 먼저 늘리지 않는다.
- 개별 route는 공통 shell과 작업대 primitive가 정해진 뒤 얕게 붙인다.
- dashboard는 analytics 페이지가 아니라 전체 운영 구조를 검증하는 ops queue로 둔다.
- Data Dragon은 픽밴/경매/전적 화면의 공유 capability이며, 특정 route 확장의 중심으로 삼지 않는다.

완료된 기반 작업:

- [x] git 저장소 초기화
- [x] `apps/web` SvelteKit + TypeScript + adapter-node scaffold
- [x] pnpm workspace 및 root check/build/lint/format script
- [x] Tailwind CSS, Bits UI, lucide-svelte 의존성 baseline
- [x] Drizzle ORM / drizzle-kit / D1-compatible SQLite schema placeholder
- [x] zod contract placeholder
- [x] `/`, `/api/[...path]`, `/oauth/[...path]`, `/dd/[...path]` route baseline

최우선 선행 작업:

```txt
Riot Data Dragon asset/catalog pipeline
```

목표:

```txt
챔피언 얼굴/icon, 이름, id, 검색용 slug를 모든 화면에서 사용할 수 있게 한다.
```

필요한 이유:

```txt
픽밴
경매 라인업
경매 결과
사용자 챔피언 선호
전적 검색
랭킹 / 프로필
```

작업:

- [x] Data Dragon latest version 조회
- [x] champion catalog 로드
- [x] champion id/key/name/idSlug 매핑
- [x] champion icon URL 생성
- [x] same-origin `/dd` asset proxy
- [x] Data Dragon image HTTP cache 최적화
- [x] mock fixture에서 champion catalog 사용
- [x] champion search helper
- [x] champion face/icon component
- [x] app shell / top bar / workflow rail / operations status rail baseline
- [x] shared layout primitives baseline

v3 참고:

```txt
../mookbot_v3/packages/core/src/datadragon/client.ts
../mookbot_v3/packages/core/src/datadragon/types.ts
../mookbot_v3/packages/core/src/datadragon/index.ts
../mookbot_v3/apps/api/src/http/champions.ts
../mookbot_v3/apps/api/src/http/_helpers.ts
../mookbot_v3/apps/web/src/lib/api/pickban.ts
../mookbot_v3/apps/web/src/lib/components/mook/champion-hover-summary.svelte
```

원칙:

- 프론트 초기 mock 단계에서도 실제 champion icon이 보여야 한다.
- Data Dragon catalog는 UI 구현의 필수 fixture로 취급한다.
- 챔피언 얼굴/icon이 없는 픽밴/전적/경매 화면은 완료로 보지 않는다.
- 구현은 v3 코드를 참고하되 새 구조에 맞게 작게 가져온다.
- 이미지 바이너리는 Valkey에 넣지 않는다.
- 챔피언 이미지 캐싱은 HTTP 캐시 계층에서 처리한다.

Data Dragon image cache 기본안:

```txt
Browser
  -> same-origin /dd/cdn/{version}/img/champion/{file}
  -> nginx proxy cache
  -> Data Dragon CDN
```

캐시 정책:

```txt
versioned URL
  -> Cache-Control: public, max-age=31536000, immutable

latest/version lookup
  -> 짧은 TTL

champion catalog JSON
  -> memory cache
  -> 필요 시 Valkey shared cache
```

Valkey 사용 기준:

```txt
사용 가능
  dd:latest-version
  dd:champions:ko_KR:{version}

비사용
  champion image binary
  splash/loading image binary
```

Frontend 구조 목표:

```txt
개별 페이지 구현 전에 전체 제품 구조를 먼저 세운다.
```

구성:

```txt
global shell
  -> multi-guild selector
  -> operator/session state
  -> command/search area
  -> primary workflow navigation
  -> operations status rail

workspace primitives
  -> workspace header
  -> panel
  -> dense row/list/table frame
  -> status pill
```

목표:

```txt
SvelteKit 기반 운영 콘솔의 전체 페이지를 실제 운영 가능한 디자인 수준으로 먼저 완성한다.
```

데이터 전략:

```txt
1. mock fixture data
2. fake API
3. real API
```

원칙:

- 처음에는 backend 없이 mock fixture로 모든 화면과 workflow를 구현한다.
- mock data는 실제 API 계약으로 전환하기 쉬운 형태로 관리한다.
- 실제 DB/API가 없어도 운영자가 화면 흐름을 끝까지 검토할 수 있어야 한다.
- 단, 화면을 page 단위로 깊게 만들기 전에 공통 shell과 route frame이 먼저 안정되어야 한다.

첫 화면:

```txt
사용자가 소속된 guild dashboard
```

guild 진입 규칙:

```txt
소속 guild가 1개
  -> 해당 /g/{guildId} dashboard로 바로 진입

소속 guild가 여러 개
  -> 우선순위 규칙에 따라 기본 guild dashboard로 진입
  -> 사용자는 기본 진입 guild 순서를 설정할 수 있어야 함
```

추후 협의:

```txt
guild 우선순위 결정 방식
  - 최근 사용 guild
  - 즐겨찾기 guild
  - 직접 정렬한 guild 순서
  - operator 권한 guild 우선
```

Data Schema 반영 사항:

```txt
guilds.is_externally_public
  -> guild 외부 사용자에게 공개 화면/공개 전적 접근을 허용할지 결정
  -> 기본값 false
  -> 운영 화면, 감사 로그, 멤버 관리, 권한 정보는 공개 대상에서 제외

user_guild_preferences
  -> user_id + guild_id 기준
  -> 사용자가 속한 guild 중 최근 선택, 즐겨찾기, 정렬 순서를 저장
  -> 데이터 소유권이 아니라 개인별 진입/선택 UX 상태
```

첫 번째로 끝까지 연결할 workflow:

```txt
일반 내전
  -> 모집
  -> 엔트리 편성
  -> 픽밴
  -> 결과 입력
  -> 되돌리기
  -> 전적 반영 화면
```

UI 구현 수준:

```txt
실제 운영 가능한 디자인까지 같이 구현
```

원칙:

- wireframe 수준으로 끝내지 않는다.
- 재사용 가능한 컴포넌트는 필요할 때 만든다.
- 디자인 시스템 자체에 과도하게 투자하지 않는다.
- Card 중첩형 AI 디자인은 배제한다.
- Tailwind CSS + Bits UI + lucide-svelte 기준을 유지한다.

작업:

- [ ] 프로젝트 scaffold
- [x] Riot Data Dragon asset/catalog pipeline baseline
- [x] app shell
- [x] shared layout primitives
- [ ] guild 진입 / 기본 guild 선택 흐름
- [ ] guild 외부 공개 여부 설정
- [ ] user 단위 guild 선택/즐겨찾기/정렬 상태
- [ ] guild dashboard
- [ ] 일반 내전 화면: 샘플 컴포넌트 작성
- [ ] 경매 내전 화면: 샘플 컴포넌트 작성
- [ ] 전적 검색 화면: FAB mock 작성
- [ ] 관리자 / 되돌리기 화면
- [x] mock fixture data 구조
- [ ] fake API 전환 준비

현재 작성 현황:

```txt
기준 커밋
  50b22ff feat: update app shell and workflow samples

작성 완료
  - 전역 app shell / 상단 navbar / 검색 패널
  - 루트 화면의 shell 검증용 정리
  - Noto Sans KR 기반 전역 typography
  - light/dark color token baseline
  - gpt-image 기반 logo/favicon asset
  - Data Dragon champion catalog/icon helper
  - Riot position icon asset
  - component sample route: /dev/components
  - FAB sample route: /dev/fab
  - FAB shell, 미니게임 modal, 전적검색 modal
  - 코인토스 / 룰렛 / 사다리 미니게임 mock 구현
  - 빠른 전적검색 mock UI
  - 엔트리 편성 DND mock board
  - 일반 내전 결과 입력 mock board
  - 결과 입력의 사이드 선택 / 픽밴 입력 / 승자 선택 단계
  - 픽밴 챔피언 입력 autocomplete
  - 결과 입력 undo/redo UI
  - 결과 입력 strict flag

샘플 수준
  - 일반 내전 엔트리 편성
  - 일반 내전 픽밴/결과 입력
  - 경매 입찰 배정 샘플
  - 빠른 전적검색 FAB
  - 미니게임 FAB

아직 실제 route/workflow에 연결되지 않음
  - 모집 목록과 모집 상세
  - 실제 guild dashboard
  - 실제 series 결과 저장
  - 실제 전적 반영
  - 실제 guild별 내전 전적 검색
  - 실제 Riot API 연동
  - fake API layer
  - Backend/API/DB 상태 전이
  - Bot command/deeplink 연동
```

현재 작성 파일 묶음:

```txt
App shell / root
  apps/web/src/lib/components/layout/*
  apps/web/src/routes/+page.svelte

개발 확인 route
  apps/web/src/routes/dev/components/+page.svelte
  apps/web/src/routes/dev/fab/+page.svelte

FAB
  apps/web/src/lib/components/fab/*
  apps/web/src/lib/fixtures/quickRecordSearch.ts

Workflow samples
  apps/web/src/lib/components/workflow/*
  apps/web/src/lib/fixtures/workflow.ts
  apps/web/src/lib/fixtures/resultEntry.ts

Assets
  apps/web/static/brand/*
  apps/web/static/riot/positions/*
```

주의:

- 현재 `/dev/components`와 `/dev/fab`는 검증용 화면이다.
- mock fixture 기준으로 UX와 디자인을 검증하는 단계이며, 저장/조회는 실제 API에 연결되어 있지 않다.
- 일반 내전 결과 입력은 컴포넌트 샘플로 작성되었고, 실제 `/series/[id]/result` route에 연결된 상태가 아니다.
- 엔트리 편성 DND와 결과 입력 컴포넌트는 현재 디자인 기준점으로 유지한다.
- 이후 route 연결 시 샘플 컴포넌트를 그대로 옮기기보다 API 계약과 load/action 구조에 맞춰 감싸야 한다.

완료 기준:

```txt
Frontend의 모든 계획 페이지가 실제 운영 가능한 디자인 수준으로 구현되어야 한다.
Backend 없이 mock fixture data만으로 전체 사용자 workflow를 검토할 수 있어야 한다.
Frontend 전체 페이지 구현이 완료되기 전에는 다음 단계로 넘어가지 않는다.
그 이후에 Bot Phase로 넘어간다.
```

## Phase 2: Bot

목표:

```txt
Discord에서 대부분의 운영 흐름을 시작하고, 세부 작업은 Web으로 연결한다.
```

결정된 사항:

```txt
명령어 이름: v3 기존 이름 유지
초기설정 입력: guild 이름 + 관리 권한 이름
Bot 역할: 작업 시작점 / 알림 / 웹 딥링크 제공
세부 작업: 특별한 단발성 명령을 제외하고 Web에서 처리
되돌리기: 엔트리 되돌리기 같은 고정된 운영 단계 복구
```

초기설정:

```txt
/초기설정 guild_name admin_role_name
  -> guild 표시 이름 설정
  -> 관리 권한 role 이름 설정
  -> dashboard link 제공
```

Bot/Web 경계:

```txt
Bot에서 시작
  -> 모집 생성
  -> 참가/취소
  -> 목록/조회
  -> 빠른 알림
  -> 웹 딥링크

Web에서 처리
  -> 엔트리 편성
  -> 픽밴
  -> 경매 입찰/라인업/결과
  -> 관리자 화면
  -> 전적 검색 상세
```

되돌리기 범위:

```txt
general-purpose undo/redo 아님
정해진 운영 단계의 되돌리기만 지원

예:
  엔트리 편성 되돌리기
  마지막 게임 결과 되돌리기
  경매 입찰 단계 되돌리기
  라인업 확정 취소
```

작업:

- [ ] command scaffold
- [ ] v3 command 이름 유지
- [ ] guild setup command
- [ ] 일반 내전 command/components
- [ ] 경매 내전 command/components
- [ ] 사용자 / 전적 command
- [ ] 되돌리기 command

완료 기준:

```txt
v3의 주요 사용자-facing command 이름이 유지된다.
Bot command는 운영 흐름을 시작하고 필요한 경우 Web route로 넘긴다.
초기설정은 guild 이름과 관리 권한 이름만 받는다.
```

## Phase 3: Backend

목표:

```txt
Frontend와 Bot에서 확정된 계약을 기준으로 상태 전이와 저장소를 구현한다.
```

작업:

- [ ] DB schema
- [ ] Discord OAuth2 session
- [ ] guild permission
- [ ] 일반 내전 domain
- [ ] 경매 내전 domain
- [ ] 전적 검색 domain
- [ ] 고정 단계 revert
- [ ] audit log

메모:

```txt
추후 협의
```

## 보류

```txt
세부 기술 선택
배포 방식
DB provider
cache provider
테스트 전략
마이그레이션 전략
```
