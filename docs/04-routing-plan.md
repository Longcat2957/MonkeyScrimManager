# MonkeyBot Routing Plan

작성일: 2026-07-10

## 목적

라우팅 계획을 프론트엔드, 백엔드, Bot 책임으로 완전히 분리한다.

기존 `../mookbot_v3`는 SvelteKit web route, Fastify API route, Discord bot command/component handler가 함께 맞물려 동작한다. 새 프로젝트에서는 각 계층의 라우팅 책임을 문서 단위로 분리해, 구현 시 어느 앱의 책임인지 명확하게 유지한다.

## 분리 문서

- [Frontend Routing](routing/frontend.md): SvelteKit page/proxy route, 화면 URL, route별 데이터 요구.
- [Backend Routing](routing/backend.md): Fastify HTTP API, OAuth/session, WebSocket topic, internal bot notify endpoint.
- [Bot Routing](routing/bot.md): Discord slash command, button/select interaction, 웹 딥링크 생성 규칙.
- [Expected Workflows](06-expected-workflows.md): bot 설치부터 일반 내전, 경매, 전적 검색, 관리자 작업까지의 예상 사용 흐름.
- [Realtime Collaboration](07-realtime-collaboration.md): 동시 편집, revision, WebSocket invalidate, Valkey, 충돌 복구 기준.
- [Component Entrypoints](08-component-entrypoints.md): 재사용 UI의 import 위치, route 배치, props와 목업 상태.

## 전체 원칙

- Discord bot은 모집과 알림의 시작점이다.
- Web route는 운영자가 작업을 이어가는 딥링크다.
- Backend route는 상태 전이, 권한, audit log, WebSocket invalidate의 source of truth다.
- Frontend route는 화면과 same-origin proxy만 담당한다.
- Bot handler는 Discord 상호작용과 메시지 갱신에 집중하고, 복잡한 상태 전이는 Backend API/Core로 위임한다.
- 인증은 오직 Discord OAuth2만 지원하며, 별도 회원가입은 만들지 않는다.
- Discord Activity route는 만들지 않는다.

## 기존 프로젝트 기준

참고한 기존 위치:

- `../mookbot_v3/apps/web/src/routes`
- `../mookbot_v3/apps/api/src/http`
- `../mookbot_v3/apps/bot/src/commands`
- `../mookbot_v3/docs/v2-feature-inventory.md`
- `../mookbot_v3/docs/v3-roadmap.md`

## 우선순위 요약

### P0

- Frontend: `/`, `/g/[guildId]`, `/oauth/[...path]`, `/api/[...path]`, `/dd/[...path]`, `/g/[guildId]/recruitments/[id]/entry`, `/g/[guildId]/series/[id]/pick-ban`, `/g/[guildId]/series/[id]/result`
- Backend: auth/session, recruitment, series, games, champions, healthz, WebSocket
- Bot: `/내전모집`, 일반 모집 button/select, 엔트리/픽밴/결과 웹 링크

### P1

- Frontend: `/g/[guildId]/series`, `/g/[guildId]/auction/**`, `/users`, `/users/[discordId]`, `/g/[guildId]/leaderboard`, `/me`
- Backend: auction recruitment/tournament/match, users/profile, leaderboard, riot accounts
- Bot: 경매 모집/진행 command, 사용자 조회 command, Riot 조회 command

### P2

- Frontend: `/g/[guildId]/admin`, `/admin/status`, `/admin/perms`, `/tools/minigame`
- Backend: logs, deep health/admin status, permission diagnostics, cleanup/admin endpoints
- Bot: 관리자 command, audit log link, maintenance command
