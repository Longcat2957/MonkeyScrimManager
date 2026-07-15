# MonkeyBot Routing Plan

작성일: 2026-07-15

## 목적

현재 운영 중인 라우팅을 프론트엔드, 백엔드, Bot 책임으로 분리해 기록한다.

기존 `../mookbot_v3`는 SvelteKit web route, Fastify API route, Discord bot command/component handler가 함께 맞물려 동작한다. 새 프로젝트에서는 SvelteKit frontend-only, Axum API, Discord bot의 책임을 문서 단위로 분리한다.

## 분리 문서

- [Frontend Routing](routing/frontend.md): SvelteKit page/proxy route, 화면 URL, route별 데이터 요구.
- [Backend Routing](routing/backend.md): Axum HTTP API, OAuth/session, WebSocket topic, internal bot notify endpoint.
- [Backend Development Plan](09-backend-development-plan.md): PostgreSQL schema, transaction, 단계별 구현과 완료 기준.
- [Bot Routing](routing/bot.md): Discord slash command, button/select interaction, 웹 딥링크 생성 규칙.
- [Operating Workflows](06-expected-workflows.md): bot 설치부터 일반 내전, 경매, 전적 검색, 관리자 작업까지의 현재 사용 흐름.
- [Realtime Collaboration](07-realtime-collaboration.md): 동시 편집, revision, WebSocket invalidate, Valkey, 충돌 복구 기준.
- [Component Entrypoints](08-component-entrypoints.md): 재사용 UI의 import 위치, route 배치, props와 목업 상태.
- [Current Operating Contract](12-current-operating-contract.md): 구현 기준의 현재 운영 흐름과 제공 범위.
- [Code Review Plan](13-code-review-plan.md): 현재 구현 기준의 검토 순서, 범위, 검증 게이트.

## 전체 원칙

- Discord bot은 모집과 알림의 시작점이다.
- Web route는 운영자가 작업을 이어가는 딥링크다.
- Backend route는 상태 전이, 권한, audit log, WebSocket invalidate의 source of truth다.
- Frontend route는 화면과 same-origin proxy만 담당한다.
- Bot handler는 Discord 상호작용과 메시지 갱신에 집중하고, 복잡한 상태 전이는 Axum application service로 위임한다.
- 인증은 오직 Discord OAuth2만 지원하며, 별도 회원가입은 만들지 않는다.
- Discord Activity route는 만들지 않는다.

## 기존 프로젝트 기준

참고한 기존 위치:

- `../mookbot_v3/apps/web/src/routes`
- `../mookbot_v3/apps/api/src/http`
- `../mookbot_v3/apps/bot/src/commands`
- `../mookbot_v3/docs/v2-feature-inventory.md`
- `../mookbot_v3/docs/v3-roadmap.md`

## 현재 운영 Surface

- Frontend: 공개 `/landing`, guild dashboard, 일반 내전 모집/엔트리/픽밴/결과, 경매 모집/입찰/브래킷/라인업/match/결과,
  records, leaderboard, `/me`, guild admin, static documents가 실제 route로 배치되어 있다.
- Backend: Discord OAuth/session, guild-scoped workflow, Riot account/records, admin, notification outbox,
  제한적 WebSocket invalidate를 Axum이 소유한다.
- Bot: 현재 command registry에는 `/초기설정`, `/등록`, `/내전모집`과 경매 모집/운영 명령만 등록되어 있다.
  정확한 목록은 [Current Operating Contract](12-current-operating-contract.md)를 따른다.

새 route 또는 command는 기존 P0/P1/P2 표를 선행 계약으로 삼지 않는다. 먼저 실제 workflow와
`apps/*` 구현을 정하고, 이 문서와 route별 문서를 같은 변경에서 갱신한다.
