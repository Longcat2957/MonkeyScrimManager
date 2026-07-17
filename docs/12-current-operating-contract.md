# MonkeyBot Current Operating Contract

작성일: 2026-07-15

## 문서 지위

이 문서는 현재 동작하는 MonkeyBot의 운영 계약을 요약한다. 구현과 기존 계획 문서가 충돌하면 다음 순서로
판단한다.

1. 현재 runtime 구현: `apps/web/src/routes`, `apps/web/src/lib/api`, `apps/api/src/http/mod.rs`,
   `apps/bot/src/commands/index.ts`
2. 이 문서와 route별 계약 문서
3. 과거 계획/roadmap 및 `mookbot_v3` 참고 구현

문서는 구현되지 않은 기능을 현재 제공 기능처럼 적지 않는다. 기능이 바뀌면 구현 변경과 같은 작업에서 이
문서와 해당 route 문서를 함께 갱신한다.

## 시스템 경계

```txt
Discord Bot / Web browser
  -> Axum API
  -> PostgreSQL
  -> transactional outbox -> Valkey Pub/Sub -> WebSocket invalidate
```

- Discord OAuth session이 Web 인증의 유일한 수단이다.
- 비로그인 사용자가 `/`에 처음 접근하면 공개 `/landing` 소개 화면으로 이동한다. 랜딩의 Discord 로그인은
  기존 OAuth callback 뒤 `/`의 대표 guild 분기를 다시 사용한다.
- Discord user ID가 사용자 식별자이며, Discord 전역/서버별 표시명은 갱신 가능한 표시 속성이다.
- `guildId`가 운영 데이터와 MMR, 내전 전적의 tenant 경계다.
- Riot 계정은 PUUID로 연결하지만, 사용자는 Riot ID(`GameName#TagLine`)만 입력한다.
- PostgreSQL이 상태와 revision의 source of truth다. WebSocket과 Valkey는 상태를 확정하지 않는다.
- Web과 Bot은 같은 API 상태 전이를 사용한다. Bot은 PostgreSQL에 직접 접근하지 않는다.

## 일반 내전

```txt
내전 모집 생성
  -> OPEN 모집의 참가자 수집
  -> 정원 도달 후 모집 마감
  -> 엔트리 편성
  -> 시리즈 생성
  -> 픽/밴 및 세트 결과 입력
  -> 완료 결과와 guild MMR 반영
```

- Discord `/내전모집`과 Web `/g/{guildId}/series`는 같은 모집 생성 contract를 사용한다.
- 생성 시 BO1/BO3/BO5와 하드 피어리스 여부를 확정하고, 이후 series에 immutable rule snapshot으로 복사한다.
- Discord 모집 카드와 Web 운영자의 수동 참가자 추가는 하나의 canonical roster를 바꾼다.
- 엔트리 편성은 `CLOSED` 모집에서만 진행한다. 모집을 다시 열기, draft 초기화, 모집 취소는 backend capability가
  허용할 때만 표시한다.
- 픽/밴 입력은 현재 세트의 실제 진영을 먼저 선택한 뒤 진행한다. 일괄 입력은 픽/밴 단계에서만 보이며, 하드
  피어리스일 때 이전 확정 세트의 pick 재사용을 막는다.
- 확정 game, 점수, MMR, 결과 화면은 API 응답 snapshot을 기준으로 갱신한다. UI가 MMR이나 다음 상태를 계산하지
  않는다.
- 현재 recovery control은 모집 취소/재개, entry reset, series 복귀/삭제, 마지막 세트 되돌리기, BO3/BO5 조기
  종료다. 정확한 가능 여부는 각 snapshot의 capability가 결정한다.

## 경매 내전

```txt
경매 모집 생성
  -> canonical roster 확정
  -> 팀장/시작 포인트 설정
  -> BIDDING 드래프트
  -> 브래킷 확정
  -> 라인업 확정
  -> match별 픽/밴 및 결과 입력
  -> 토너먼트 결과와 guild MMR 반영
```

- Web `/g/{guildId}/auction`과 Discord `/경매모집`은 같은 경매 모집 aggregate를 생성한다.
- 10인은 결승 BO1/BO3/BO5, 20인은 준결승 BO1/BO3/BO5와 결승 BO1/BO3/BO5를 각각 모집 시점에 확정한다.
  하드 피어리스도 같은 immutable rule snapshot에 저장한다.
- 모집 roster는 Discord 참가와 active operator의 Web 수동 추가/제거가 공유한다. dummy user도 같은 member ID와
  picker 경로를 사용하며 별도 운영 UI를 만들지 않는다.
- `CLOSED` 모집에서 captain과 시작 point를 저장하고 tournament를 만든다. `BIDDING`에서 후보 draw, 낙찰,
  유찰, 수동 배치와 최근 draft action 되돌리기를 처리한다.
- 브래킷은 팀 배치와 lock만 담당한다. 10인은 결승 match로, 20인은 두 준결승의 20명 라인업을 함께 확정하는
  route로 진행한다. lineup이 모두 lock되기 전에는 어느 준결승도 결과 입력으로 진행하지 않는다.
- match 결과 입력은 일반 내전과 같은 픽/밴 입력 보드를 재사용한다. 경매의 일괄 입력은 아직 확정되지 않은
  현재 세트 local draft에만 반영한 후 기존 game submit 한 번으로 확정한다.
- 현재 recovery control은 최근 입찰 action 되돌리기, 브래킷 확정 해제, 입찰 단계 복귀, lineup unlock,
  마지막 확정 game 되돌리기와 권한·상태가 허용하는 Web 또는 Bot 경매 취소/조기 종료다. backend capability와
  lifecycle 검증을 우회하지 않는다.

## 실시간 동기화

모든 mutation은 HTTP snapshot, `expectedRevision`, CSRF, 필요한 경우 `Idempotency-Key`로 확정한다.
WebSocket은 authoritative state patch가 아니라 invalidate 신호만 전달한다.

| 화면                                     | WebSocket 대상            | 범위                                      |
| ---------------------------------------- | ------------------------- | ----------------------------------------- |
| 일반 내전 엔트리 편성                    | active guild member       | 모집/entry draft 두 topic invalidate      |
| 일반 시리즈 픽/밴                        | 현재 active operator      | 진행 중 series invalidate                 |
| 경매 모집 상세                           | active operator           | OPEN/CLOSED roster invalidate             |
| 경매 입찰                                | `BIDDING` active operator | tournament draft invalidate               |
| 경매 setup, bracket, lineup, match, 결과 | 사용하지 않음             | HTTP snapshot과 revision conflict refetch |

- reconnect, browser focus, `resync_required`, 더 높은 revision invalidate 뒤에는 HTTP GET으로 최신 snapshot을
  다시 읽는다.
- 일반 길드 member의 read-only 화면, records, leaderboard, admin, result ledger는 WebSocket을 열지 않는다.
- `409 revision_conflict`는 자동 merge나 재시도가 아니라 최신 snapshot 반영 후 사용자가 다시 작업하는 방식이다.

## 사용자, 전적, 관리

- `/me`는 선호 챔피언, Riot 계정, 대표 길드, 개인정보를 실제 API snapshot으로 관리한다. 선호 챔피언은 lane별
  ordered 목록이며 저장은 `PUT /api/me/champion-preferences/{lane}`를 사용한다.
- `/records`는 외부 Riot 전적, `/g/{guildId}/records`는 guild 내부 내전 전적이다. 두 결과를 하나의
  snapshot으로 섞지 않는다.
- guild records의 연결 Riot ID 표시는 사용자의 privacy projection을 따른다. 내부 guild 게임과 MMR은 개인
  privacy setting으로 숨기지 않는다.
- guild admin은 members, settings, seasons, data transfer, audit logs를 현재 운영 범위에 둔다.
  operations/revert 전용 admin 탭과 web bulk registration은 노출하지 않는다.
- `/g/{guildId}/admin/data`의 기본 경로는 active operator가 `GUILD_FULL_SNAPSHOT` V1 plain JSON으로
  멤버·시즌·모집·시리즈·경매·픽밴·게임·MMR 원장·감사 기록을 함께 내보내고 가져온다. Session/OAuth secret,
  Discord role/channel catalog·설정, cache/outbox는 포함하지 않는다. `MEMBER_SEASON_MMR_SEED` API는 현재 시즌
  MMR만 교환하는 하위 호환 계약으로 유지한다.
- full data import는 source와 target Discord guild ID가 달라도 허용한다. Target의 기존 closed season 이력은
  보존하고 현재 active season의 전적·MMR·진행 도메인만 비어 있어야 한다. 모든 archive guild 참조를 target
  ID로 매핑하고 source Discord 게시물 연결은 제거한다. Archive의 active season을 참조하는 모든 row는 target의
  현재 season ID로 매핑하며, 전역 모집/시리즈/경매 display number는 기존 target 번호 뒤로 재배치한다. Preview는 PostgreSQL SAVEPOINT에서
  이 매핑 결과를 실제 제약으로 검사하고 모든 domain 변경을 rollback한다. Apply는 archive와 target digest를
  다시 검증한 뒤 한 transaction으로만 반영하며 partial success가 없다. 동일 사용자의 Riot 계정이나 lane별
  선호 챔피언이 이미 MonkeyBot에 있으면 현재 전역 프로필을 보존하고, 비어 있는 사용자/lane에만 archive 값을 보충한다.
- `mookbot_v3` D1/Valkey는 read-only snapshot과 명시적 정제/변환 report를 거쳐 같은
  `GUILD_FULL_SNAPSHOT`으로 만든다. 변환·preview와 실제 apply는 분리하며 operator 승인 전에는 apply하지 않는다.
- dummy user 기능은 `MONKEYBOT_DUMMY_USERS_ENABLED=true`인 non-production service-admin 도구다. 더미
  사용자는 실제 DB entity이지만 로그인/Discord role/운영 principal이 될 수 없다.

## Discord 애플리케이션 환경 경계

- 로컬 PC와 VPS dev는 `MonkeyBot[dev]` 한 애플리케이션을 사용하고, VPS main만 `MonkeyBot`을 사용한다.
- 로컬 OAuth redirect는 `http://localhost:5173/oauth/callback`, VPS dev는
  `https://dev.monkeybot.lol/oauth/callback`, VPS main은 `https://monkeybot.lol/oauth/callback`으로 고정한다.
- API와 Bot은 환경별 공통 `MONKEYBOT_DISCORD_APPLICATION_ID`를 사용한다. OAuth client secret과 Bot token은
  각각의 secret runtime 파일에만 두며 deployment env나 저장소에 넣지 않는다.
- 로컬과 VPS dev의 command scope는 `guild`이고 `MONKEYBOT_BOT_DEV_GUILD_ID`가 필수다. VPS main의 scope는
  `global`이며 dev guild ID를 설정하지 않는다.
- Bot은 로그인한 토큰의 application ID가 설정값과 같은지 확인하기 전에는 readiness를 열거나 명령을
  등록하지 않는다. 로컬 helper는 guild 등록만 제공하며 dev application의 과거 global 명령 삭제는 별도
  명시적 작업으로 한 번만 실행한다.
- 실제 파일 배치, preflight, 자격증명 교체 순서는 `docs/15-discord-environment-operations.md`를 따른다.

## 현재 Bot 명령 등록

`apps/bot/src/commands/index.ts`에 현재 등록된 명령만 사용자에게 제공한다.

| 명령            | 역할                                                 |
| --------------- | ---------------------------------------------------- |
| `/초기설정`     | 운영 role, 필수 모집 채널, 선택 feedback 채널을 설정 |
| `/등록`         | 현재 guild membership과 Riot ID 연결                 |
| `/내전모집`     | 일반 내전 모집 생성                                  |
| `/경매모집`     | 10/20인 경매 모집과 rule snapshot 생성               |
| `/경매인원추가` | 운영자의 경매 모집 대상 참가자 추가                  |
| `/경매인원삭제` | 운영자의 경매 모집 참가자 제거                       |
| `/경매목록`     | active/completed tournament 조회                     |
| `/경매강제취소` | 경매 모집/연결 tournament의 안전 취소                |
| `/경매조기종료` | 진행 중 tournament 조기 종료                         |

`/내전인원추가`, `/내전목록`, `/새시즌`, `/도움말`, `/전적`, `/랭킹` 등을 포함한 나머지 문서상 명령은 현재
command registry에 없으므로 제공 기능으로 안내하지 않는다.

## 현재 제외 범위

- Riot Spectator/현재 게임
- custom match에서 KDA/CS를 필수 입력으로 받는 흐름
- CRDT, OT, offline mutation queue, 범용 undo/redo
- WebSocket을 통한 full state patch 또는 client-side MMR/낙찰 계산
- web bulk registration, 범용 maintenance dashboard, 일반 사용자용 dummy user 관리
- 일반 guild operator의 mookbot_v3 D1/Valkey dump 또는 전체 내전/경매 history import
