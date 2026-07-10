# MonkeyScrimManager Expected Workflows

작성일: 2026-07-10

## 목적

이 문서는 새 프로젝트에서 예상되는 실제 사용 흐름을 정리한다.

구현 세부 설명이 아니라, Discord bot, Web console, API/Core, DB가 어떤 순서로 맞물리는지 한눈에 보기 위한 문서다.

핵심 전제:

```txt
Discord guild = 운영 단위
Discord OAuth2 = 웹 사용자 인증
guild_id = 모든 운영 데이터의 경계
Bot = 시작점 / 알림 / 빠른 명령
Web = 실제 운영 작업대
API/Core = 상태 전이와 권한의 source of truth
```

## 전체 운영 흐름

```txt
1. 길드 운영자가 Discord bot 설치
2. Discord에서 /초기설정 실행
3. guild dashboard 활성화
4. 운영자와 유저가 Discord OAuth2로 웹 접속
5. 모집 / 편성 / 픽밴 / 결과 / 경매 / 전적 / 로그 사용
6. 모든 기록은 guild_id 기준으로 분리 저장
```

제품의 기본 흐름:

```txt
Discord command
  -> Backend API
  -> Core domain
  -> DB / KV / Riot API
  -> Bot message update
  -> Web dashboard invalidate
```

되돌리기 기본 흐름:

```txt
운영자가 정해진 작업 수행
  -> 필요 시 revert checkpoint 기록
  -> 화면에는 되돌릴 수 있는 고정 작업 표시
  -> 운영자가 실수 발견
  -> 되돌리기 실행
  -> checkpoint 상태로 복구
  -> 관련 화면 invalidate
```

되돌리기의 우선순위:

```txt
1. game result / MMR 변경 되돌리기
2. 경매 입찰 / 수동 배치 / 라인업 확정 되돌리기
3. 모집 취소 / series 생성 / bracket 생성 되돌리기
4. 관리자 destructive action 복구
```

원칙:

- 주요 작업 화면에는 최근 작업과 되돌리기 진입점이 있어야 한다.
- 범용 undo/redo를 만들지 않는다.
- 엔트리 편성, 마지막 게임, 경매 입찰 단계, 라인업 확정 취소처럼 정해진 지점만 되돌린다.
- audit log는 추적용이고, 실제 복구 기준은 revert checkpoint다.

## 1. 길드 설치 / 초기 설정

사용자:

- 길드 운영자

흐름:

```txt
운영자가 bot 설치
  -> guilds 생성 또는 복구
  -> status = INSTALLED

운영자가 /초기설정 guild_name admin_role_name 실행
  -> 실행자를 guild_members에 등록
  -> 초기 operator로 기록
  -> guild 표시 이름 저장
  -> 관리 권한 role 이름 저장
  -> 기본 season 생성
  -> status = ACTIVE

Bot이 dashboard link 제공
  -> /g/{guildId}
```

결과:

```txt
guilds
guild_members
guild_operator_roles
guild_settings
seasons
admin_audit_log
```

원칙:

- 같은 bot이 여러 guild에 설치되어도 각 guild는 독립 운영된다.
- bot 재설치 시 기존 `guild_id` 데이터는 복구 가능해야 한다.
- `/초기설정`은 회원가입이 아니라 guild 운영 공간 활성화다.
- `/초기설정`은 guild 이름과 관리 권한 이름만 받는다.

## 2. 웹 로그인 / 권한 확인

사용자:

- 운영자
- 참가자
- 기록을 확인하려는 일반 유저

흐름:

```txt
사용자가 /g/{guildId} 접속
  -> Discord OAuth2 login
  -> users upsert
  -> guild_members 확인
  -> Discord guild membership / role 확인
  -> dashboard 접근 또는 권한 없음 표시
```

원칙:

- 인증은 Discord OAuth2만 사용한다.
- 별도 회원가입 화면은 없다.
- Riot ID 등록은 계정 생성이 아니라 Discord user에 Riot account를 연결하는 작업이다.
- operator 권한은 guild마다 다를 수 있다.

## 3. 일반 내전 운영

사용자:

- 운영자
- 참가자

시작점:

```txt
Discord /내전모집
```

흐름:

```txt
/내전모집
  -> recruitments(guild_id) 생성
  -> Discord 모집 메시지 발행

참가자 button/select interaction
  -> 참가 / 취소 / 선호 라인 저장
  -> Discord 모집 메시지 갱신
  -> dashboard invalidate

정원 도달 또는 운영자 마감
  -> Bot이 entry editor link 제공
  -> /g/{guildId}/recruitments/{id}/entry

운영자가 Web에서 엔트리 편성
  -> entry draft 저장
  -> series 생성
  -> /g/{guildId}/series/{id}/pick-ban

운영자가 픽밴 / 결과 입력
  -> games 생성
  -> game_stats / picks / bans 저장
  -> MMR 반영
  -> mmr_changes 기록
  -> 필요 시 revert checkpoint 기록

시리즈 완료
  -> 결과 화면
  -> Bot 종료 카드 발행
  -> 전적 검색과 프로필에 반영
```

핵심 화면:

```txt
/g/{guildId}
/g/{guildId}/recruitments
/g/{guildId}/recruitments/{id}/entry
/g/{guildId}/series
/g/{guildId}/series/{id}/pick-ban
/g/{guildId}/series/{id}/result
```

원칙:

- 일반 내전은 MMR에 반영한다.
- `games.guild_id`가 전적과 MMR 변경의 경계다.
- 게임 결과와 MMR 변경은 반드시 되돌릴 수 있어야 한다.
- 되돌리기/강제 삭제/조기 종료는 checkpoint 또는 audit log를 남긴다.

## 4. 경매 내전 운영

사용자:

- 운영자
- 참가자

시작점:

```txt
Discord /경매모집
```

흐름:

```txt
/경매모집
  -> auction_recruitments(guild_id) 생성
  -> Discord 경매 모집 메시지 발행

참가자 button interaction
  -> 참가 / 취소 저장
  -> Discord 메시지 갱신
  -> dashboard invalidate

정원 도달 또는 운영자 시작
  -> /g/{guildId}/auction/recruitments/{id}/draft

운영자가 Web에서 경매 시작
  -> auction_tournaments 생성
  -> 팀장 선택
  -> 포인트 배정
  -> 후보 draw / bid / 낙찰 / 수동 배치
  -> 필요한 단계에 revert checkpoint 기록

브래킷 시작
  -> auction_matches 생성
  -> /g/{guildId}/auction/tournaments/{id}/bracket

경기 시작 전 라인업 확정
  -> match 선택
  -> 양 팀 5명의 라인 입력
  -> TOP / JUNGLE / MID / ADC / SUPPORT 모두 확정
  -> 라인업 확정 후 경기 시작 가능

경매 경기 결과 입력
  -> games 생성
  -> game_stats / picks / bans 저장
  -> MMR 반영 없음

토너먼트 완료
  -> 결과 화면
  -> Bot 종료 알림
  -> 전적 검색에는 이벤트 기록으로 표시
```

핵심 화면:

```txt
/g/{guildId}/auction
/g/{guildId}/auction/recruitments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/bracket
/g/{guildId}/auction/tournaments/{id}/result
```

원칙:

- 경매 내전은 기록은 남기지만 MMR에는 반영하지 않는다.
- 경매 match는 게임 시작 전에 양 팀 라인업을 반드시 확정해야 한다.
- 라인업이 비어 있거나 중복 role이 있으면 결과 입력으로 넘어갈 수 없다.
- 경매 진행 상태는 UI local state가 아니라 DB 원장을 기준으로 복구 가능해야 한다.
- 입찰, 수동 배치, 라인업 확정, 결과 입력은 이전 단계로 되돌릴 수 있어야 한다.
- 입찰 되돌리기/취소/강제 종료는 checkpoint 또는 audit log를 남긴다.

## 5. 사용자 등록 / Riot 계정 연결

사용자:

- 참가자
- 운영자

시작점:

```txt
Discord /등록 riot_id
Web /me/riot-accounts
```

흐름:

```txt
/등록 riot_id
  -> Discord user 확인
  -> users upsert
  -> riot_accounts 연결
  -> Riot profile refresh

Web Riot 계정 관리
  -> Discord OAuth2 session 확인
  -> Riot 계정 추가 / 삭제 / 메인 지정 / 갱신
```

원칙:

- `/등록`은 회원가입이 아니다.
- Riot 계정은 global user에 연결된다.
- guild별 전적과 MMR은 Riot account가 아니라 `guild_id + user_id` 기준으로 분리된다.

## 6. 전적 검색 / 사용자 조회

사용자:

- 운영자
- 참가자
- 기록을 확인하려는 일반 유저

시작점:

```txt
Web /g/{guildId}/records
Bot /전적 [riot_id|user]
Bot /내전기록 [user]
```

흐름:

```txt
검색어 입력
  -> Discord user / Riot ID / guild display name 검색
  -> 현재 guild의 내부 내전 기록 조회
  -> games / stats / picks / bans / mmr_changes 집계
  -> Riot 외부 전적 요약은 보조로 조회
```

결과 구성:

```txt
내부 내전 전적
  승패
  라인별 MMR
  최근 내전
  챔피언 사용
  pick/ban 기록
  일반 / 경매 참여 이력

Riot 외부 요약
  Riot ID
  랭크
  최근 게임
  screening report
```

원칙:

- 전적 검색의 기본 source of truth는 guild 내부 `games` 원장이다.
- Riot 외부 전적은 내부 MMR 계산에 직접 쓰지 않는다.
- 같은 Discord user라도 guild가 다르면 전적과 MMR은 다르게 보일 수 있다.

## 7. 대시보드 운영

사용자:

- 운영자

흐름:

```txt
/g/{guildId}
  -> 열린 모집
  -> 마감 대기 모집
  -> 진행 중 시리즈
  -> 진행 중 경매
  -> 최근 완료 기록
  -> 처리 필요한 운영 이벤트
```

대시보드의 역할:

```txt
분석 화면이 아니라 운영 queue
다음 작업으로 이동하는 진입점
guild 상태와 권한 문제를 빨리 발견하는 화면
```

원칙:

- 대시보드는 카드 나열형 장식 화면이 아니다.
- 운영자가 지금 처리해야 하는 항목을 먼저 보여준다.
- WebSocket invalidate 후 필요한 데이터만 다시 조회한다.

## 8. 관리자 / 안전장치

사용자:

- 운영자
- admin

흐름:

```txt
운영자 명령 또는 admin screen
  -> 권한 확인
  -> 되돌릴 수 있는 작업인지 판정
  -> 필요 시 revert checkpoint 기록
  -> destructive action 실행
  -> admin_audit_log 기록
  -> dashboard / 관련 화면 invalidate
```

대상 작업:

```txt
MMR 수동 수정
시즌 결과 리셋
오래된 내전 정리
모집 / 시리즈 / 경매 강제 삭제
취소 모집 복구
게임 결과 되돌리기
권한 진단
audit log 조회
```

원칙:

- destructive action은 audit log 없이 실행하지 않는다.
- destructive action에 무리한 undo/redo를 붙이지 않는다.
- 되돌리기 불가능한 작업은 UI와 Bot에서 명시적으로 표시한다.
- admin action도 guild target이 있으면 반드시 `guild_id`를 기록한다.
- 실제 삭제보다 soft-delete 또는 상태 전이를 우선한다.

## 9. Bot 제거 / 재설치

사용자:

- 길드 운영자

흐름:

```txt
Bot이 guild에서 제거됨
  -> guilds.status = LEFT
  -> left_at 기록
  -> 운영 데이터 보존

Bot 재설치
  -> 기존 guilds 복구
  -> status = INSTALLED 또는 ACTIVE
  -> 운영자 재확인
  -> dashboard 재사용
```

원칙:

- bot 제거는 데이터 삭제가 아니다.
- purge는 별도 관리자 작업으로만 가능해야 한다.
- 재설치 후 기존 전적, 시즌, MMR, 경매 기록은 복구 가능해야 한다.

## MVP Workflow

초기 버전에서 반드시 끝까지 연결되어야 하는 흐름:

```txt
Bot 설치
  -> /초기설정
  -> Discord OAuth2 웹 로그인
  -> /내전모집
  -> 참가/취소/라인 선택
  -> 엔트리 편성
  -> 픽밴
  -> 결과 입력
  -> MMR 반영
  -> /g/{guildId}/records에서 전적 확인
```

경매 MVP:

```txt
/경매모집
  -> 참가/취소
  -> 팀장/포인트 설정
  -> 입찰
  -> 브래킷
  -> match별 라인업 확정
  -> 결과 입력
  -> MMR 미반영 기록 저장
```

운영 안전 MVP:

```txt
권한 확인
revert checkpoint
audit log
게임 결과 되돌리기
경매 입찰/라인업 되돌리기
soft-delete
bot 재설치 복구
```
