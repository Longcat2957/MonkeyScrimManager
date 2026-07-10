# MonkeyScrimManager

SvelteKit fullstack 기반으로 새로 작성하는 multi-guild 내전 운영 시스템.

## 먼저 볼 것

```txt
1. IMPLEMENTATION_ROADMAP.md
2. docs/01-purpose.md
3. docs/06-expected-workflows.md
4. docs/05-db-schema.md
```

## 현재 결정된 것

```txt
개발 순서: Frontend -> Bot -> Backend
주 앱: SvelteKit fullstack
인증: Discord OAuth2 only
디자인: Tailwind CSS + Bits UI + lucide-svelte
배포: VPS
DB: Cloudflare D1
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
경매 내전은 MMR 미반영
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
docs/routing/frontend.md        Frontend route
docs/routing/backend.md         Backend API route
docs/routing/bot.md             Bot command/component route
```

## 작업할 때 기준

```txt
세부 구현 전에는 IMPLEMENTATION_ROADMAP.md부터 수정
구조/흐름 변경은 docs/06-expected-workflows.md 또는 docs/05-db-schema.md에 반영
라우팅 변경은 docs/routing/*에 반영
디자인 결정은 docs/03-design-direction.md에 반영
```
