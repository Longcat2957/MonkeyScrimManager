# MonkeyBot Expected Workflows

작성일: 2026-07-12

## 목적

이 문서는 새 프로젝트에서 예상되는 실제 사용 흐름을 정리한다.

구현 세부 설명이 아니라 Discord bot, Web console, Axum application service, PostgreSQL이 어떤 순서로 맞물리는지 한눈에 보기 위한 문서다.

핵심 전제:

```txt
Discord guild = 운영 단위
Discord OAuth2 = 웹 사용자 인증
guild_id = 모든 운영 데이터의 경계
Bot = 시작점 / 알림 / 빠른 명령
Web = 실제 운영 작업대
Axum API/PostgreSQL = 상태 전이와 권한의 source of truth
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
  -> Axum application/domain service
  -> DB / Valkey / Riot API
  -> Bot message update
  -> 해당되는 Web 편집 workspace invalidate
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
- series/tournament/bracket 생성 되돌리기는 기존 row를 CANCELLED로 보존한다. 다시 생성할 때는
  새 domain ID를 발급한다.
- 일반 내전은 어느 lifecycle 상태에서도 운영자가 현재 작업을 수정하고, 직전 단계로 복구하고,
  삭제 요청을 수행할 수 있어야 한다. 이는 임의 시점 복원이 아니라 각 단계의 명시 lifecycle
  command이며, 종속 game/MMR은 역순 `VOIDED`/reversal 뒤에만 상위 aggregate를 취소한다.
- 위 일반 내전 복구성은 **필수 목표 계약**이다. 현재 entry-draft API가 존재한다는 사실만으로
  `CONVERTED` 이후 series/game cascade rollback까지 구현되었다고 해석하지 않는다. 해당 command가
  OpenAPI와 router에 추가되기 전에는 그 상태의 수정/복구/삭제 기능을 제공한다고 표시하지 않는다.

## 1. 길드 설치 / 초기 설정

사용자:

- 길드 운영자

흐름:

```txt
운영자가 bot 설치
  -> guilds 생성 또는 복구
  -> status = INSTALLED

운영자가 /초기설정 [운영자역할] [모집채널] [길드이름] [엔트리채널] [결과알림채널] [운영로그채널] 실행
  -> 실행자를 guild_members에 등록
  -> 초기 operator로 기록
  -> guild 표시 이름 저장
  -> 관리 권한 role 이름 저장
  -> 필수 모집 채널과 선택적 Discord feedback 채널 저장
  -> 기본 season 생성
  -> status = ACTIVE

Bot이 dashboard link 제공
  -> /g/{guildId}
```

결과:

```txt
guilds
guild_members
guild_config
seasons
admin_audit_log
```

원칙:

- 같은 bot이 여러 guild에 설치되어도 각 guild는 독립 운영된다.
- bot 재설치 시 기존 `guild_id` 데이터는 복구 가능해야 한다.
- `/초기설정`은 회원가입이 아니라 guild 운영 공간 활성화다.
- `/초기설정`은 필수 Discord 관리 role과 모집 채널, 선택적 guild 표시 이름/엔트리/결과/운영 로그 채널을 받는다.
- `/초기설정`은 `INSTALLED` 또는 `SETUP_REQUIRED` 상태에서 설정 row가 없을 때 한 번만 실행한다.
- 동일 Discord interaction의 재전송은 저장된 성공 응답을 재생하지만, 설정 완료 뒤 새 interaction으로
  다시 실행하면 `guild_already_configured`로 거절하고 guild, member, config, season을 변경하지 않는다.
- 활성화 이후 이름과 role 변경은 guild 관리 화면의 revision 기반 설정 API를 사용한다.

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
Web /g/{guildId}/series 내전 모집
```

흐름:

```txt
/내전모집
  -> BO1/BO3/BO5, 하드 피어리스 선택
  -> 선택 규칙을 가진 recruitments(guild_id) 생성
  -> Discord 모집 메시지 발행

Web 내전 모집 (ACTIVE guild member)
  -> BO1/BO3/BO5와 하드 피어리스 선택
  -> 모집 채널 설정 여부 확인
  -> 선택 규칙을 포함한 recruitments(guild_id), 빈 entry draft, RECRUITMENT_MESSAGE_CREATE outbox row를 한 transaction으로 생성
  -> /g/{guildId}/recruitments/{id} 엔트리 모집 화면 이동
  -> Bot worker가 configured 모집 채널에 /내전모집과 같은 메시지 게시
  -> typed ack이 Discord channel/message ID를 recruitment에 원자적으로 bind
  -> Discord 모집 메시지에서 참가 / 취소 / 선호 라인 수집
  -> 엔트리 편성 완료 후 series 생성 시 모집 규칙을 immutable snapshot으로 복사

Web 운영자 수동 참가자 추가
  -> OPEN 모집에서 ACTIVE guild member 선택
  -> backend가 operator/정원/중복/멤버 상태를 transaction 안에서 검증
  -> participant, audit, invalidate, bound message UPDATE notification 기록

Web 모집 취소 (생성자 또는 운영자)
  -> OPEN 또는 CLOSED recruitment를 CANCELLED로 soft transition
  -> participant/entry assignment/draft history 보존, audit/invalidate/bound message DELETE notification 기록
  -> initial CREATE가 아직 bind 전이면 typed ack이 현재 status를 확인해 DELETE를 후속 적재
  -> 취소 모집은 목록, detail, entry-draft read에서 제외

참가자 button/select interaction
  -> 참가 / 취소 / 선호 라인 저장
  -> Discord 모집 메시지 갱신
  -> 열려 있는 엔트리 모집 화면 invalidate

정원 도달 후 운영자가 Discord 또는 Web에서 엔트리 편성 시작
  -> recruitment CLOSED, entry editor link 제공
  -> /g/{guildId}/recruitments/{id}/entry

운영자가 Web에서 엔트리 편성
  -> entry draft 저장
  -> series 생성
  -> bound 모집 메시지를 시리즈 진행 상태로 갱신하고, 설정된 엔트리 채널에는 확정 roster와 픽/밴 링크를 비동기 게시
  -> /g/{guildId}/series/{id}/pick-ban

운영자가 픽밴 / 결과 입력
  -> games 생성
  -> game participants / picks / bans 저장
  -> MMR 반영
  -> mmr_ledger_entries 기록
  -> 필요 시 revert checkpoint 기록

시리즈 완료
  -> 결과 화면
  -> configured 결과 채널에 최종 스코어/세트 결과/결과 링크가 담긴 Bot 종료 카드 비동기 발행
  -> 전적 검색과 프로필에 반영
```

핵심 화면:

```txt
/g/{guildId}
/g/{guildId}/recruitments/{id}
/g/{guildId}/recruitments/{id}/entry
/g/{guildId}/series
/g/{guildId}/series/{id}/pick-ban
/g/{guildId}/series/{id}/result
```

원칙:

- 일반 내전은 MMR에 반영한다.
- `games.guild_id`가 전적과 MMR 변경의 경계다.
- 게임 결과와 MMR 변경은 반드시 되돌릴 수 있어야 한다.
- 되돌리기/강제 취소/조기 종료는 checkpoint 또는 audit log를 남긴다.
- 일반 내전은 `OPEN`, `CLOSED`, `CONVERTED`, `IN_PROGRESS`, `COMPLETED`, `CANCELLED` 어느
  시점에서도 active operator가 수정, 직전 운영 단계 복귀, 삭제를 요청할 수 있어야 한다.
  완료된 결과는 덮어쓰거나 물리 삭제하지 않고 마지막 game부터 ledger tail을 검증해 void/reversal한
  뒤에만 series와 source recruitment를 복구한다.
- 이는 일반 내전 lifecycle의 필수 완성 기준이며 현재 공개된 route 목록이 아니다. 구현되지 않은
  converted/series/game cascade action은 UI와 Bot에서 실행 가능하다고 노출하지 않는다.

## 4. 경매 내전 운영

사용자:

- 운영자
- 참가자

시작점:

```txt
Discord /경매모집
Web /g/{guildId}/auction 경매 모집
```

흐름:

```txt
Discord /경매모집 또는 Web 경매 모집 (ACTIVE guild member)
  -> 10인(TEN_PERSON) 또는 20인(TWENTY_PERSON) 형식 선택
  -> 공통 하드 피어리스와 결승 BO1/BO3/BO5 선택
  -> 20인은 4강 BO1/BO3/BO5도 별도로 선택
  -> 같은 auction recruitment create command 호출
  -> 현재 season/counter를 잠그고 OPEN auction_recruitments, 빈 setup draft/team slot을 한 transaction으로 생성
  -> configured 모집 채널의 AUCTION_RECRUITMENT_MESSAGE_CREATE outbox로 동일 경매 모집 메시지를 발행
  -> Bot worker typed acknowledgement가 Discord channel/message ID를 recruitment에 원자적으로 bind
  -> /g/{guildId}/auction/recruitments/{id} 모집 화면 이동

참가자 button/select interaction
  -> 참가 lane 선택 / 참가 / 취소 저장
  -> Discord 메시지 갱신
  -> 경매 모집 aggregate invalidate

Web 현재 active operator의 참가자 관리
  -> ACTIVE guild member(Discord 또는 development dummy)를 선택하고 declared lane을 명시
  -> Discord 참가와 같은 canonical roster, 현재 시즌 lane MMR snapshot, capacity/duplicate 검증을 사용
  -> Discord 메시지 갱신과 operator 경매 모집 workspace invalidate

정원 도달 또는 운영자 시작
  -> OPEN 모집을 CLOSED로 전환
  -> /g/{guildId}/auction/recruitments/{id}/draft에서 확정 참가자 확인
  -> format에 맞는 2개 또는 4개 팀장과 팀별 시작 포인트 저장
  -> 모집에서 저장한 BO/하드 피어리스 규칙은 이 단계에서 수정하지 않음

경매 tournament 생성
  -> 저장된 auction_setup_drafts revision을 검증해 auction_tournaments 생성
  -> captain/team/candidate/initial point 원장 생성
  -> 후보 draw / bid / 낙찰 / 수동 배치
  -> 필요한 단계에 revert checkpoint 기록

브래킷 시작
  -> 모집 rules snapshot을 tournament로 복사하고 auction_matches 생성
  -> 10인은 결승 규칙, 20인은 두 4강과 결승의 각 규칙을 적용
  -> /g/{guildId}/auction/tournaments/{id}/bracket

경기 시작 전 라인업 확정
  -> /g/{guildId}/auction/matches/{matchId}에서 match 선택
  -> 양 팀 5명의 라인 입력
  -> TOP / JUNGLE / MID / BOTTOM / SUPPORT 모두 확정
  -> 라인업 확정 후 경기 시작 가능

경매 경기 결과 입력
  -> games 생성
  -> game participants / picks / bans 저장
  -> MMR 반영

토너먼트 완료
  -> 결과 화면
  -> Bot 종료 알림
  -> 전적 검색에는 이벤트 기록으로 표시
```

핵심 화면:

```txt
/g/{guildId}/auction
/g/{guildId}/auction/recruitments/{id}
/g/{guildId}/auction/recruitments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/draft
/g/{guildId}/auction/tournaments/{id}/bracket
/g/{guildId}/auction/matches/{matchId}
/g/{guildId}/auction/tournaments/{id}/result
```

원칙:

- 경매 내전도 결과 입력 시 guild MMR에 반영한다.
- 경매 match는 게임 시작 전에 양 팀 라인업을 반드시 확정해야 한다.
- 라인업이 비어 있거나 중복 role이 있으면 결과 입력으로 넘어갈 수 없다.
- 경매 진행 상태는 UI local state가 아니라 DB 원장을 기준으로 복구 가능해야 한다.
- 입찰, 수동 배치, 라인업 확정, 결과 입력은 이전 단계로 되돌릴 수 있어야 한다.
- 입찰 되돌리기/취소/강제 종료는 checkpoint 또는 audit log를 남긴다.
- Web과 Discord는 모집 생성 주체만 다르며, format, season, display number, participant capacity,
  setup draft와 Discord 모집 메시지의 canonical state를 별도로 만들지 않는다.
- tournament와 match의 stage/BO/하드 피어리스는 모집 때 저장한 rules snapshot에서만 파생한다. Web route,
  query parameter, local count가 새 규칙이나 다음 단계를 추론하지 않는다.

## 5. 사용자 등록 / Riot 계정 연결

사용자:

- 참가자
- 운영자

시작점:

```txt
Discord /등록 riot_id
Web /me
```

흐름:

```txt
/등록 riot_id
  -> 현재 Discord guild와 실행자 Discord user id(snowflake) 확인
  -> users를 Discord user id 기준으로 upsert
  -> GameName, TagLine, platform 입력 검증
  -> Riot Account-v1에서 PUUID/canonical Riot ID 확인
  -> (guild_id, Discord user id) guild_members 소속 등록
  -> PUUID 기준 riot_accounts를 global user에 연결
  -> 길드 소속과 Riot 계정 연결이 모두 반영된 경우에만 성공 응답

Web Riot 계정 관리
  -> Discord OAuth2 session 확인
  -> GameName, TagLine, platform으로 Riot 계정 추가
  -> 삭제 / 메인 지정 / 갱신
  -> mutation 응답의 최신 계정 목록으로 화면 교체
```

원칙:

- `/등록`은 회원가입이 아니다.
- `/등록`은 DM에서 실행할 수 없으며 interaction이 발생한 현재 Discord guild에만 소속을 등록한다.
- Discord user의 canonical identity는 닉네임이 아니라 Discord user id(snowflake)다. `guild_members`의
  identity는 `(guild_id, user_id)`이고 guild nickname은 갱신 가능한 표시 속성이다.
- Riot 계정의 canonical identity는 PUUID이며 global user에 연결된다. GameName과 TagLine 변경으로
  새로운 사용자나 길드 소속을 만들지 않는다.
- Riot resolver가 성공한 뒤 길드 소속 등록과 Riot 계정 연결을 하나의 짧은 transaction으로 확정한다.
  어느 한쪽만 반영된 상태로 `/등록` 성공을 응답하지 않는다.
- 같은 Discord interaction 재처리나 같은 사용자의 반복 실행은 멱등적이어야 한다. 이미 ACTIVE인
  길드 소속과 같은 Riot 계정 연결을 중복 생성하지 않는다.
- 관리자가 해당 guild에서 INACTIVE로 전환한 기존 member는 `/등록`으로 ACTIVE로 되돌리지 않는다.
  명령을 거절하고 운영자의 명시적 재활성화를 요구한다.
- 같은 Discord user가 다른 guild에서 `/등록`하면 전역 user/Riot 계정은 재사용하고 새
  `(guild_id, user_id)` 소속만 등록한다. 전적과 MMR은 계속 guild별로 분리한다.
- Bot과 Web은 같은 Riot ID resolver와 account linking application service를 사용한다.
- 사용자나 frontend는 PUUID와 regional routing을 입력하지 않는다. Backend가 Riot 응답에서 확정한다.
- guild별 전적과 MMR은 Riot account가 아니라 `guild_id + user_id` 기준으로 분리된다.
- Web `/me`의 Riot 계정 관리는 guild context가 없으므로 새로운 `guild_members` 소속을 만들지 않는다.
- 사용자는 `/me`에서 외부 프로필, Discord 사용자와 Riot ID의 연결 관계, 길드 밖 custom match 전적
  접근 범위를 설정한다.
- 같은 guild 내부의 경기 기록과 MMR은 guild 원장이므로 개인 공개 설정으로 숨기지 않는다.
- public Riot ID와 외부 Riot 전적 검색은 사용자 privacy flag로 제한하지 않으며 Discord 연결 관계와
  PUUID를 노출하지 않는다.

## 6. 전적 검색 / 사용자 조회

사용자:

- 운영자
- 참가자
- 기록을 확인하려는 일반 유저

시작점:

```txt
Web /g/{guildId}/records
Bot /내전기록 [user]

Web /records
Bot /전적 [riot_id]
```

흐름:

```txt
길드 전적 검색어 입력
  -> Discord ID / guild display name / 공개가 허용된 연결 Riot ID exact match
  -> guild member와 전역 선호 챔피언 조회
  -> show_riot_ids가 허용한 연결 Riot 계정 목록만 projection
  -> 현재 guild의 내부 내전 기록 조회
  -> games / participants / stats / picks / bans / mmr_ledger_entries 집계

Riot 전적 검색어 입력
  -> GameName#TagLine을 UI/Bot adapter에서 GameName, TagLine으로 분리
  -> platform과 함께 공통 Riot ID resolver 호출
  -> Riot profile / rank / champion aggregate / recent match 조회
```

결과 구성:

```txt
내부 내전 전적
  사용자 표시명 / Discord avatar
  포지션별 선호 챔피언
  공개가 허용된 연결 Riot ID
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
```

원칙:

- 전적 검색의 기본 source of truth는 guild 내부 `games` 원장이다.
- Riot 외부 전적은 내부 MMR 계산에 직접 쓰지 않는다.
- 같은 Discord user라도 guild가 다르면 전적과 MMR은 다르게 보일 수 있다.
- `/g/{guildId}/records`와 `/records`는 API와 화면을 분리한다. 한 응답에 내부 원장과 Riot match를
  섞지 않는다.
- 길드 전적 profile의 Riot ID는 외부 match를 합치는 데이터가 아니라 Discord 사용자와 Riot 계정의
  연결 관계를 표시하는 privacy-filtered identity다.
- screening report와 `/전적검토`는 MVP 이후 별도 계약이 생길 때만 추가한다.

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
- 대시보드는 WebSocket을 구독하지 않고 navigation, focus, 명시 refresh에서 다시 조회한다.

## 8. 동시 편집 / 실시간 동기화

대상:

- 엔트리 편성
- 픽밴 입력
- 경매 드래프트와 입찰
- 경매 match 라인업과 결과 입력

흐름:

```txt
HTTP GET snapshot + revision
  -> 작은 operation 단위 mutation + expectedRevision
  -> DB transaction commit
  -> WebSocket invalidate
  -> HTTP GET 재조회 또는 409 Conflict 처리
```

원칙:

- DB/API를 source of truth로 사용한다.
- 전체 draft 덮어쓰기와 last-write-wins를 사용하지 않는다.
- 연결 복구 후 현재 aggregate를 다시 조회한다.
- 경매 낙찰과 결과 입력은 하나의 DB transaction에서 확정한다.

전체 계약과 필수 장애 테스트는 [Realtime Collaboration](07-realtime-collaboration.md)을 따른다.

## 9. 관리자 / 안전장치

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
  -> 변경한 domain aggregate에 허용된 topic이 있을 때만 관련 편집 화면 invalidate
  -> dashboard/admin은 다음 HTTP 조회에서 반영
```

대상 작업:

```txt
MMR 수동 수정
현재 시즌 종료와 새 시즌 생성
오래된 내전 CANCELLED 정리
모집 / 시리즈 / 경매 강제 취소
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
- 시즌 전환은 기존 시즌을 CLOSED로 보존하고 새 ACTIVE 시즌을 만든다. 전적/MMR 원장을
  초기화하거나 삭제하지 않는다.

## 10. Bot 제거 / 재설치

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

경매 tournament workflow:

```txt
Discord /경매모집 또는 Web /g/{guildId}/auction 경매 모집
  -> 참가/취소
  -> 팀장/포인트 설정
  -> 입찰
  -> 브래킷
  -> match별 라인업 확정
  -> 결과 입력
  -> MMR 반영 기록 저장
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
