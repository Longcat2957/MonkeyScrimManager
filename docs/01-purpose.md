# MonkeyBot Purpose

작성일: 2026-07-11

## 목적

MonkeyBot은 `../mookbot_v3`의 내전 운영 로직을 계승하되, 제품 구조와 사용자 경험을 새로 설계하는 신규 프로젝트다. 목표는 기존 v1, v2, v3를 거치며 누적된 레거시 UI, Activity 전제, 임시 디자인 패턴, 사용 빈도가 낮은 코드 경로를 정리하고, Discord 모집과 웹 운영 콘솔 중심의 작고 명확한 시스템을 만드는 것이다.

이 프로젝트는 `mookbot_v3`를 그대로 포크해 다듬는 리팩토링이 아니다. v3는 기능 기준선과 검증된 도메인 로직의 참고 구현이며, 새 프로젝트는 필요한 계약과 동작만 선별해 다시 작성한다.

## 배경

기존 mookbot 계열은 버전이 올라가면서 다음 문제가 누적됐다.

- v1, v2, v3의 화면 구조와 디자인 판단이 겹쳐져 실제 운영 흐름보다 장식적/중복적 UI가 많아졌다.
- Discord Activity 기반 UX에서 일반 웹 콘솔로 넘어오며 남은 용어, 상태, 코드 경로가 있다.
- React Activity, 여러 UI 프레임워크, route-local Tailwind 보정 등이 시기별로 섞이며 UI 원칙이 흐려졌다.
- 봇, API, 웹, DB 패키지 사이에 검증된 로직도 있지만, 기능 이식 과정에서 임시 호환 코드와 사용 빈도 낮은 관리 플로우가 늘어났다.
- 운영자가 실제로 반복 사용하는 모집, 편성, 픽밴, 결과 입력, 경매, 사용자 확인, 감사 로그 흐름이 제품 구조의 중심에 충분히 고정되어 있지 않다.

## 제품 정의

MonkeyBot은 League of Legends 내전 운영을 위한 Discord 연동 운영 콘솔이다.

주 사용자는 내전 운영자다. 사용자는 Discord에서 모집을 만들고 참가자를 모은 뒤, 웹 콘솔에서 팀 편성, 픽밴, 게임 결과, 경매 드래프트, 브래킷, 사용자 기록, 감사 로그를 처리한다.

제품의 역할은 다음처럼 분리한다.

- Discord bot: 모집 생성, 참가/취소, 라인 선호 입력, 운영자 명령, 알림, 종료 카드 발행.
- Web console: 운영자가 실제 작업을 처리하는 기본 화면. 대시보드, 편성, 픽밴, 경매, 결과, 사용자/랭킹/로그를 담당.
- API: 인증, 권한, 도메인 상태 전이, audit log, WebSocket invalidate, bot notify를 담당.
- Axum backend: 인증, 권한, application service, SQLx repository, MMR/ELO, Riot API, WebSocket을 담당.

## 인증 정책

MonkeyBot의 인증은 오직 Discord OAuth2만 지원한다.

- 별도 회원가입은 만들지 않는다.
- 이메일/비밀번호 계정은 만들지 않는다.
- 자체 username/password 로그인은 만들지 않는다.
- 소셜 로그인 제공자를 추가하지 않는다.
- 앱 내부 사용자는 Discord OAuth2 session에서 확인된 Discord user id를 기준으로 식별한다.
- `/등록` 명령과 Riot 계정 관리는 앱 회원가입이 아니라, 이미 Discord로 식별된 사용자에게 Riot ID를 연결하는 기능이다.

## 계승할 핵심 기능

`../mookbot_v3`에서 반드시 기능 기준선으로 삼을 영역은 다음과 같다.

- Discord 사용자 등록과 Riot ID 연결.
- 사용자 정보, 내전 기록, 랭킹, 내부/외부 전적 검색, 전적검토 조회.
- 일반 내전 모집, 참가/취소, 선호 라인 선택, 운영자 강제 참가/삭제.
- 일반 내전 엔트리 편성, pick/ban draft, 게임 결과 입력, MMR 반영, 시리즈 완료.
- 경매 내전 모집, 팀장 선출, 포인트 배정, 입찰, 수동 배치, 브래킷, 경매 결과.
- 사용자 프로필, MMR 히스토리, 챔피언 선호, Riot 계정 관리.
- 관리자 기능: MMR 수정, 시즌 전환, 오래된 내전 취소 정리, 강제 취소/복구, 되돌리기, audit log.
- WebSocket topic invalidate 후 재조회하는 실시간 동기화 방식.
- soft-delete, audit log, 운영자 권한, 주요 상태 전이 보존 정책.

## 버릴 것

새 프로젝트에서는 다음을 기본적으로 계승하지 않는다.

- Discord Activity SDK와 Activity session/instance 중심 사고.
- `apps/activity` React UI와 그 화면 구조.
- v1/v2/v3에서 누적된 장식적 카드 대시보드, nested card, 마케팅형 hero, 과한 metric tile.
- Skeleton UI, daisyUI class, Activity iframe 전제의 스타일/문구.
- 사용자가 거의 접근하지 않는 임시 debug UI 또는 migration-only 코드 경로.
- route마다 임시로 맞춘 색상, spacing, badge, card utility.
- 도메인 의미 없이 만들어진 wrapper, helper, compatibility layer.

기존 코드 재사용은 허용하지만, 기준은 "복사 비용이 낮은가"가 아니라 "새 구조에서도 유지할 가치가 명확한가"다.

## 설계 원칙

### 운영 흐름 우선

첫 화면과 주요 route는 운영자가 다음에 무엇을 해야 하는지 바로 보이게 만든다. 대시보드는 analytics가 아니라 운영 queue다. 엔트리, 픽밴, 경매 화면은 작업대가 먼저고 요약 카드는 보조다.

### 도메인 로직 보존

MMR, game 기록, revert checkpoint, audit log, soft-delete 같은 운영 신뢰성 규칙은 v3를 참고하되 현재 프론트 계약과 PostgreSQL 제약에 맞춰 다시 정의한다. series와 recruitment, auction tournament와 recruitment는 별도 ID와 unique foreign key로 연결한다.

### 되돌리기 우선

운영자는 실수할 수 있다. 하지만 범용 undo/redo를 만들지는 않는다. 엔트리 편성, 마지막 게임 결과, 경매 입찰 단계, 라인업 확정 취소처럼 정해진 운영 지점에서만 되돌리기를 제공한다.

### 작은 표면

기능을 줄인다는 뜻이 아니라, 코드와 화면의 표면적을 작게 둔다. 자주 쓰는 흐름을 명확하게 구현하고, 낮은 빈도의 관리 기능은 명령형/로그형 UI로 충분히 제공한다.

### 계약 중심 재작성

API response, mutation request, WebSocket topic, domain state를 먼저 문서화하고 타입으로 고정한다. 웹과 봇은 이 계약 위에 얇게 구현한다.

### 실시간 상태의 소유권

동시 편집 화면의 source of truth는 DB와 API다. WebSocket은 변경 사실만 알리고 Valkey는 전달 계층과 재생성 가능한 임시 상태만 담당한다. revision, 충돌, 재연결, 경매 원자성의 세부 기준은 [Realtime Collaboration](07-realtime-collaboration.md)을 따른다.

### 봇은 슬림하게

Discord bot은 Discord 상호작용과 알림에 집중한다. 복잡한 도메인 전이는 Axum application service로 이동시키고, 봇 안에는 command parsing, permission check, message rendering, component handler만 남긴다.

### 웹은 콘솔답게

웹은 낮은 장식성, 높은 정보 밀도, 안정적인 row/table/list/workspace 위계를 가진다. 카드 안의 카드, 큰 radius, shadow, generic SaaS layout은 기본값이 아니다.

## 성공 기준

1. 운영자는 Discord에서 모집을 만들고 웹 콘솔로 이동해 일반 내전과 경매 내전을 끝까지 처리할 수 있다.
2. 일반 내전 결과는 기존 MMR/통계 정책과 호환된다.
3. 경매 내전 결과도 guild MMR과 통계에 반영된다.
4. 운영자와 사용자는 Discord user 또는 Riot ID로 guild 내부 전적을 검색할 수 있다.
5. 기존 v3 DB 데이터를 마이그레이션하거나 읽어올 수 있는 경로가 있다.
6. 봇 명령어의 사용자-facing 이름과 주요 UX는 유지하되 내부 구현은 더 작고 테스트 가능하다.
7. 웹 UI는 Activity/legacy 디자인을 답습하지 않고, route별 작업 목적이 화면 구조에 직접 반영된다.
8. 배포 단위는 bot, api, web, DB, Valkey로 명확히 나뉜다.
9. 새 기능 추가 전 `typecheck`, 핵심 domain/API 테스트, route별 smoke 검증이 통과한다.

## Non-goals

초기 버전에서 다음은 목표가 아니다.

- 완전히 새로운 게임 운영 규칙 도입.
- MMR 알고리즘의 전면 교체.
- Riot API 기반 자동 결과 판정.
- 모바일 전용 앱 수준의 UX.
- v3의 모든 관리/디버그 화면을 1차 버전에 그대로 복원.
- 디자인 시스템을 독립 패키지처럼 과하게 일반화.
- Discord OAuth2 외의 회원가입/로그인 방식.

## 기준 문서

참고 기준선은 다음 문서와 코드다.

- `../mookbot_v3/docs/v2-feature-inventory.md`
- `../mookbot_v3/docs/v3-roadmap.md`
- `../mookbot_v3/docs/v3-db-migration-plan.md`
- `../mookbot_v3/docs/modern-ui-direction.md`
- `../mookbot_v3/apps/api/src/http`
- `../mookbot_v3/apps/bot/src/commands`
- `../mookbot_v3/packages/core/src/db`
- `../mookbot_v3/packages/core/src/mmr`
- `../mookbot_v3/packages/core/src/riot`
