# Core Fix Plan

작성일: 2026-07-19

## 범위

이 문서는 RiotAPI 조사 이후 남은 core fix 대기 항목만 고정한다. 아래 항목은 아직 구현하지 않는다.

## 1. 경매 lineup 확정 후 balance-confirm Discord 메시지 누락

- 증상: 경매 lineup이 결정됐지만 Discord balance-confirm 알림이 발송되지 않는다.
- 확인 지점: `auction_match_lineup_confirmation_snapshot`, notification outbox 생성, Bot claim/ack 경로,
  Discord channel 설정.
- 완료 기준: lineup lock transaction이 한 번만 알림 outbox를 만들고, Bot이 같은 API snapshot 기준으로 메시지를
  발송/ack 한다. 중복 lock 또는 retry는 중복 Discord 메시지를 만들지 않는다.
- 검증: repository transaction test, `/internal/notifications/claim` endpoint test, Bot notification unit test.

## 2. 경매 내전/토너먼트 조기 종료 GUI

- 증상: Discord command 경로에는 조기 종료가 있으나 Web 운영 GUI에서 같은 recovery action을 완료할 수 없다.
- 확인 지점: auction tournament/match snapshot capability, `cancel-preview`, `cancel`, existing command contract,
  operator-only UI action placement.
- 완료 기준: Web은 backend capability가 허용할 때만 조기 종료 control을 보이고, preview/confirmation 후 기존
  backend transition을 호출한다. UI가 lifecycle 검증을 우회하지 않는다.
- 검증: targeted web API client test, route/component interaction test, backend smoke for the changed API route.

## 3. 첫 게임 blue/red side 입력 gate

- 증상: 첫 game의 실제 blue/red side 입력이 없으면 다음 process가 진행되지 못한다.
- 확인 지점: 일반 series game submit, auction match game submit, pick-ban board side preset/actual side state,
  saved game `team1_side`.
- 완료 기준: 첫 game 입력에서 실제 side를 명시적으로 저장하고, 후속 pick-ban/result flow가 저장된 side를 사용한다.
  누락/불일치 요청은 safe validation error로 중단된다.
- 검증: backend command validation tests, one series endpoint test, one auction match endpoint test, focused web
  pick-ban API/client test.

## 4. 확정된 일반 내전 엔트리를 경매 내전으로 전환

- 증상: 일반 내전 엔트리가 확정된 뒤 같은 roster를 경매 모집/토너먼트로 전환하는 운영 경로가 없다.
- 확인 지점: recruitment/entry draft status, auction recruitment creation contract, guild/season tenant boundary,
  idempotency and audit log.
- 완료 기준: CLOSED 일반 recruitment의 canonical roster를 기준으로 auction recruitment를 생성하거나 연결한다.
  이미 series로 전환된 recruitment, cross-guild data, 인원수 불일치, 중복 전환은 거부한다.
- 검증: repository transaction test, route endpoint test, frontend API adapter test. UI 연결은 별도 작은 pass로
  수행한다.
- 진행: `POST /api/guilds/{guildId}/recruitments/{id}/auction-recruitment` backend route/test와 Web
  `entryDraft` API adapter/test를 추가했다. 전용 page capability가 없어 UI 노출은 다음 작은 pass로 남긴다.
