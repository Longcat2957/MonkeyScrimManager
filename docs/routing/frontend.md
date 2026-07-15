# Frontend Routing

작성일: 2026-07-12

## 범위

이 문서는 `apps/web`의 SvelteKit route만 다룬다.

SvelteKit은 `src/routes` 디렉토리 기반의 파일 시스템 라우팅을 사용한다. 따라서 이 문서의 URL은 구현 시 `apps/web/src/routes/**/+page.svelte` 또는 `+server.ts` 구조로 대응한다.

Frontend route의 책임:

- 사용자에게 보이는 page URL.
- route별 초기 데이터 요청.
- same-origin proxy route.
- WebSocket invalidate 수신 후 화면 데이터 재조회.
- Bot이 제공한 딥링크의 landing page.

Frontend route의 비책임:

- 도메인 상태 전이의 최종 판단.
- operator/admin 권한의 최종 판단.
- audit log 기록.
- Discord interaction 처리.

Workflow root는 API의 UUID `id`와 사람용 `displayNumber`를 구분한다. URL/deep link에는 `id`를
사용하고 header, `#214`, `A-118` 같은 표시는 `displayNumber`를 사용한다. 현재 mock route ID에
포함된 숫자를 production label로 재사용하지 않는다.

인증 UI 정책:

- 로그인 시작은 Discord OAuth2로만 제공한다.
- 별도 회원가입 화면은 만들지 않는다.
- 이메일/비밀번호 로그인 화면은 만들지 않는다.
- 계정 생성 form, password reset form, email verification flow는 만들지 않는다.
- Riot 계정 연결 화면은 회원가입이 아니라 Discord 로그인 이후의 프로필 연결 기능이다.

## 실시간 편집 UI 규칙

동시 편집용 WebSocket을 사용하는 작업 화면은 현재 component 계약처럼 상태를 두 축으로 제공한다.

- connection: `connected`, `reconnecting`, `offline`.
- save: `saved`, `saving`, `dirty`, `conflict`.
- `loading`은 route/snapshot fetch 상태로 별도 처리한다.
- 연결과 저장 상태는 `RealtimeEditStateBar`로 표시한다.
- 저장하지 않은 local 입력을 invalidate만으로 덮어쓰지 않는다.
- 위험 command는 최신 revision을 확인한 뒤 실행한다.
- reconnect 후 topic 재구독과 aggregate 전체 재조회를 수행한다.

화면별 적용:

```txt
/g/[guildId]/recruitments/[id]/entry       entry draft autosave + conflict notice
/g/[guildId]/series/[id]/pick-ban          pick/ban autosave + game result command guard
/g/[guildId]/auction/recruitments/[id]      operator roster invalidate refresh
/g/[guildId]/auction/tournaments/[id]/draft operator bid invalidate refresh
```

경매 입찰은 `BIDDING` 상태의 `canManageDraft` active operator만
`guild:{guildId}:auction-tournament:{id}` invalidate를 구독한다. mutation은 authoritative HTTP snapshot,
`expectedRevision`, CSRF, idempotency key로 확정하고, WebSocket은 더 높은 revision, reconnect,
`resync_required` 후 GET refetch만 수행한다. 일반 길드 멤버는 socket을 열지 않는다.

경매 준비 설정, `BRACKET_SETUP` 이후의 브래킷, 경기 작업대는 WebSocket을 열지 않고 `409`이면 최신 snapshot을
다시 읽는다. 경매 모집 상세의 operator roster refresh는
`guild:{guildId}:auction-recruitment:{id}` invalidate를 구독한다.

일반 내전 엔트리 편성은 하나의 recruitment ID를 쓰더라도 두 revision domain을 구분한다. 제한적 realtime이
활성화되면 `guild:{guildId}:recruitment:{id}`는 `snapshot.recruitment.revision`으로,
`guild:{guildId}:recruitment-entry-draft:{id}`는 `snapshot.draft.revision`으로만 중복/역순 event를
판정한다. 두 숫자를 서로 비교하거나 대체하지 않는다.

상태 전이, autosave, 충돌 처리와 재연결 계약은 [Realtime Collaboration](../07-realtime-collaboration.md)을 따른다.

## App Shell

### `/`

파일:

- `apps/web/src/routes/+page.svelte`

목적:

- 로그인 사용자의 대표 guild를 확인해 `/g/[guildId]`로 redirect한다.
- 대표 guild가 없으면 `/me?tab=guild`의 설정 흐름으로 보낸다.

Backend 데이터:

- `GET /api/me`
- `GET /api/me/guilds`

### `/g/[guildId]`

파일:

- `apps/web/src/routes/g/[guildId]/+page.svelte`

목적:

- 현재 guild에서 입력해야 하는 편성, 픽밴, 결과, 경매 작업을 우선순위 순으로 확인한다.
- guild 전적 화면과 각 작업 route로 바로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/operations`

WebSocket: 사용하지 않는다.

### `+layout.svelte`

파일:

- `apps/web/src/routes/+layout.svelte`

목적:

- 공통 shell, navigation, auth bootstrap, toast/overlay root.
- 로그인 상태와 현재 권한 요약을 표시한다.
- 오른쪽 guild selector가 현재 `guildId` context를 표시하고 서버 전환 시 해당 guild dashboard로 이동한다.
- 모집 마감 상태는 `내전` 메뉴에 포함하고, `전적`, `랭킹`, `관리`는 현재 guild 기준 route를 사용한다.
- `관리`는 현재 guild의 operator에게만 노출하고 `/g/[guildId]/admin`으로 이동한다.
- `QuickActionFab`을 전역 배치해 미니게임과 타이머를 연다.

주의:

- route-specific 데이터를 root layout에 과하게 올리지 않는다.
- protected data는 각 page load 또는 API client에서 다시 검증한다.

### `+error.svelte`

파일:

- `apps/web/src/routes/+error.svelte`

목적:

- 404, 401, 403, 500 등 사용자-facing 오류 화면.

## Proxy Routes

### `/oauth/[...path]`

파일:

- `apps/web/src/routes/oauth/[...path]/+server.ts`

목적:

- 같은 origin에서 Discord OAuth 시작/콜백을 Backend API로 proxy한다.
- 예: `/oauth/discord/start`, `/oauth/callback`.

주의:

- 실제 OAuth 처리와 session cookie 발급은 `apps/api`가 담당한다.

### `/api/[...path]`

파일:

- `apps/web/src/routes/api/[...path]/+server.ts`

목적:

- web app에서 API를 same-origin `/api/*`로 호출하기 위한 proxy.
- local/dev 환경과 nginx 배포 환경의 호출 경로를 맞춘다.

주의:

- 도메인 API를 SvelteKit `+server.ts`로 재구현하지 않는다.
- 이 route는 proxy boundary다.
- 현재 proxy는 `/api/` prefix를 제거한 path를 `API_BASE_URL`에 붙이므로 local/dev 값은 `http://backend:port/api/`처럼 `/api/` suffix를 포함한다.
- nginx가 browser의 `/api/`를 Axum으로 직접 전달하는 배포 경로와 최종 upstream path가 같아야 한다.

### `/dd/[...path]`

파일:

- `apps/web/src/routes/dd/[...path]/+server.ts`

목적:

- Data Dragon 이미지/asset proxy.
- champion icon, spell icon, item image 등 브라우저 표시용.
- 이미지 호출량이 많으므로 browser/nginx cache를 적극 활용한다.

캐시:

```txt
/dd/cdn/{version}/...
  -> Cache-Control: public, max-age=31536000, immutable

/dd/api/versions.json 또는 latest 조회
  -> short TTL
```

원칙:

- image binary를 Valkey에 저장하지 않는다.
- versioned Data Dragon asset은 HTTP cache 계층에서 처리한다.

## Development 사용자 Routes

### `/dev/users`

파일:

- `apps/web/src/routes/dev/users/+page.server.ts`
- `apps/web/src/routes/dev/users/+page.svelte`

목적:

- PostgreSQL에 지속되는 더미 사용자를 단일/일괄 생성한다.
- Privacy, 길드 소속과 초기 lane MMR, 선호 챔피언, 합성 Riot 계정을 관리한다.
- 5/10/20인 등 기능에서 재사용할 순서 있는 사용자 group을 관리한다.
- 삭제 영향 확인, 삭제, archive/restore를 수행한다.

접근 조건:

- `MONKEYBOT_DUMMY_USERS_ENABLED=true`인 non-production API.
- Discord OAuth로 로그인한 service admin.
- AppShell의 service-admin 계정 menu와 길드 member 관리 화면의 `더미 회원` 링크에서 진입한다.

초기 데이터:

- `GET /api/dev/dummy-users?limit=100&offset=0`
- `GET /api/dev/dummy-users/catalog`
- `GET /api/dev/dummy-groups`
- `GET /api/me`
- `GET /api/me/guilds`
- Data Dragon champion catalog

Mutation은 모두 CSRF와 새 browser `Idempotency-Key`를 사용한다. Revision conflict나 삭제 차단처럼 backend가
반환한 public error code/message/request ID를 그대로 표시한다. 일괄 생성과 포함된 길드 배치/MMR은 bulk
API 한 번으로 확정한다. 선택 group 추가는 별도 revision mutation이므로 실패해도 생성 성공을 숨기지 않고
후속 배치 실패 이유를 표시한다.

이 route는 production 사용자 관리 화면이 아니다. 더미 identity는 로그인하거나 guild operator가 될 수
없으며 UI도 Discord role, mention, DM 동작을 제공하지 않는다.

## 일반 내전 Routes

### `/g/[guildId]/series`

일반 내전의 작업 목록과 완료 series를 표시한다. 상단 `내전 모집` action은 즉시 생성하지 않고 작은
설정 dialog를 연다. dialog는 `BO1 | BO3 | BO5` 세그먼트와 `하드 피어리스` boolean toggle만 제공하며,
선택값을 모집 create API에 명시적으로 전송한다. 생성 뒤에는 모집 detail로 이동한다. 규칙은 모집에서
immutable로 저장되므로 entry 또는 pick-ban 화면에 수정 control을 중복으로 두지 않는다.

### `/g/[guildId]/recruitments/[id]`

파일:

- `apps/web/src/routes/g/[guildId]/recruitments/[id]/+page.server.ts`
- `apps/web/src/routes/g/[guildId]/recruitments/[id]/+page.svelte`

목적:

- `OPEN` 모집의 엔트리 모집 현황과 참가자 roster를 확인한다.
- 헤더에만 `BOx · 일반` 또는 `BOx · 하드 피어리스` 규칙을 compact하게 표시한다. 별도 summary card나
  중복 상태 텍스트를 추가하지 않는다.
- 참가, 취소, 선호 라인 선택은 Discord 모집 메시지에서 처리한다.
- `CLOSED` 모집일 때만 엔트리 편성 route로 이동한다.
- 더미 참가자 전용 조작 UI는 두지 않는다. 더미 데이터는 persistent DB/API 개발 도구에서만 관리한다.
- `OPEN`일 때 active operator에게만 현재 guild의 ACTIVE member를 추가하는 control을 제공한다.
  대상은 Discord user와 dummy user를 같은 member picker에서 다루며 별도 더미 panel/filter를 만들지 않는다.
- `OPEN`일 때 active operator에게만 각 roster 행의 compact 제거 control을 제공한다. 제거 성공 시 API가
  반환한 snapshot으로 roster를 즉시 바꾸며, 동일 참가자에 대한 재시도는 같은 idempotency key를 사용한다.
- 정원이 정확히 찬 `OPEN` 모집에서는 active operator에게만 `엔트리 편성 시작` action을 제공한다. 성공하면
  close snapshot의 canonical entry link로 바로 이동하며, 요청 중에는 roster mutation과 모집 취소를 함께 막는다.
- 모집 생성자 또는 active operator에게만 `OPEN|CLOSED` 취소 confirmation을 제공한다. `CLOSED`에서는
  entry snapshot의 `canCancel`이 같은 권한/상태를 나타내며 entry route도 같은 DELETE command를 사용한다.
  성공하면 series 작업 목록으로 이동하며, 취소된 모집 detail 또는 entry route를 계속 렌더링하지 않는다.
- `Discord 모집 현황`, `참가 n / n`, `상태 모집 중`처럼 roster/header와 같은 정보를 반복하는 공통 summary
  block은 이 route나 다른 컴포넌트에 사용하지 않는다.

Backend 데이터:

- `GET /api/guilds/:guildId/recruitments/:id`
- operator member add: `POST /api/guilds/:guildId/recruitments/:id/members` with `{ memberId }`
- operator participant remove: `DELETE /api/guilds/:guildId/recruitments/:id/members/:memberId` with `{}`
- operator close for entry: `POST /api/guilds/:guildId/recruitments/:id/close` with `{}`
- creator/operator cancel: `DELETE /api/guilds/:guildId/recruitments/:id` with no body

WebSocket: 사용하지 않는다. Discord 변경 뒤 route 재진입 또는 새로고침은 최신 snapshot을 다시 조회한다.
Browser-created 모집의 Discord 게시는 비동기 queue로 처리되므로 create 성공 직후 message ID가 null일 수
있다. 운영자 수동 추가, 참가자 제거, 엔트리 편성 시작과 취소는 bound Discord message의 UPDATE/DELETE
delivery를 queue에 맡긴다. UI는
이를 별도 dashboard card로 중복 표현하지 않는다.

### `/g/[guildId]/recruitments/[id]/entry`

파일:

- `apps/web/src/routes/g/[guildId]/recruitments/[id]/entry/+page.svelte`

목적:

- 일반 내전 엔트리 편성.
- 참가자 팀/라인 배치.
- server가 반환한 draft snapshot을 기준으로 한 즉시 저장.
- `CLOSED` 모집을 다시 `OPEN` 모집 단계로 되돌리기.
- 현재 배치를 미리보기로 확인.

Bot 딥링크:

- Discord 일반 모집 메시지의 `엔트리 수정 시작` 액션에서 이 route로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/recruitments/:id/entry-draft`
- `PATCH /api/guilds/:guildId/recruitments/:id/entry-draft`
- `POST /api/guilds/:guildId/recruitments/:id/entry-draft/reset`
- `POST /api/guilds/:guildId/recruitments/:id/reopen`
- creator/operator cancel: `DELETE /api/guilds/:guildId/recruitments/:id` with no body

더미 참가자 배치는 persistent DB/API 개발 도구로만 처리한다. 일반 엔트리 모집과 엔트리 편성 route에는
더미 전용 panel을 렌더링하지 않는다.

route는 mount 시 `EntryDraftSnapshot`을 조회하고 loading/error/retry state를 직접 렌더링한다. 보드는
controlled `participants`/`assignments`를 받고, drag/click 이동과 초기화는 현재 draft revision,
CSRF, 새 browser idempotency key를 사용해 mutation한다. mutation 성공 후에는 optimistic local patch 대신
반환된 snapshot 전체로 화면을 교체한다. `canEdit`/`canReset`/`canReopen`이 false면 해당 action을 숨기거나
read-only로 둔다. `canCancel`은 creator 또는 active operator가 `OPEN|CLOSED` 모집을 취소할 수 있음을 뜻하며,
성공하면 series 작업 목록으로 이동한다. 모집 되돌리기 성공 시 모집 detail route로 이동한다.

route는 same-origin `/ws`에서 모집 topic과 entry-draft topic을 함께 구독하고, 두 topic이 모두 포함된
`subscribed` ack 뒤에만 `RealtimeEditStateBar`를 connected로 표시한다. 전자는 roster/status/capability
변경을 recruitment revision으로, 후자는 assignment/draft metadata 변경을 draft revision으로 처리한다. 더 큰
revision의 invalidate, reconnect, browser focus, `resync_required`에서만 authoritative snapshot을 다시 조회한다.
저장 중인 mutation은 완료될 때까지 재조회를 queue하고, server snapshot만 현재 화면의 source of truth로 사용한다.

### `/g/[guildId]/series`

파일:

- `apps/web/src/routes/g/[guildId]/series/+page.svelte`

목적:

- 상단에는 엔트리 모집, 엔트리 편성, 픽밴, 결과 입력 상태를 일반 내전 작업 목록으로 보여준다.
- 하단에는 완료된 일반 내전을 최신 순으로 보여준다.
- pick-ban 또는 result route로 이동한다.
- `RESULT`처럼 아직 game 입력이 필요한 작업은 pick-ban route로 이동하고, 완료된 row만 result
  route로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/recruitments?state=actionable`
- `GET /api/guilds/:guildId/series?state=active`
- `GET /api/guilds/:guildId/series?state=completed`

WebSocket: 사용하지 않는다.

### `/g/[guildId]/series/completed` 호환 경로

파일:

- `apps/web/src/routes/g/[guildId]/series/completed/+page.svelte`

목적:

- `/g/[guildId]/series#completed`로 redirect한다.
- 기존 Bot 메시지와 북마크 호환만 유지한다.

Backend 데이터:

- `GET /api/guilds/:guildId/series?state=completed`

### `/g/[guildId]/series/[id]/pick-ban`

파일:

- `apps/web/src/routes/g/[guildId]/series/[id]/pick-ban/+page.svelte`

목적:

- 일반 내전 픽/밴 draft.
- side 선택, game tab, 선택적 밴/픽, 승자 입력.
- 게임 결과 입력.
- 마지막 게임 되돌리기.
- BO3/BO5 조기 종료.
- 정해진 지점 되돌리기.

Bot 딥링크:

- series 생성 후 bot follow-up 또는 dashboard에서 진입한다.

Backend 데이터:

- `GET /api/guilds/:guildId/series/:id`
- draft cell: `PATCH /api/guilds/:guildId/series/:id/pickban`
- game confirm: `POST /api/guilds/:guildId/series/:id/games`
- last game revert: `DELETE /api/guilds/:guildId/series/:id/games/last`
- early completion: `POST /api/guilds/:guildId/series/:id/complete`
- `POST /api/guilds/:guildId/series/:id/revert`
- `DELETE /api/guilds/:guildId/series/:id`

page는 authoritative GET snapshot으로 frozen roster, MMR, Riot tier, 선호 챔피언, 현재 draft와 확정 set
요약을 렌더링한다. draft cell은 `expectedRevision`과 최소 operation 하나만 PATCH하며 success snapshot으로
local state를 교체한다. game 확정과 last-game rollback은 CSRF, UUID `Idempotency-Key`,
`series.revision`을 사용한다. game confirm 성공 시 response `nextHref`로 `/pick-ban` 또는 `/result`를
이동한다.

`canEdit`/`canSubmit`/`canEarlyComplete`/`canRevertLastGame`/`canRevert`/`canDelete`가 true인 active
operator만 해당 control을 렌더링한다. 조기 종료는 BO3/BO5에 한 세트 이상 확정된 경우에만 열고, 확정된
스코어와 MMR은 유지한 채 팀 승리 또는 동률 무승부를 명시 확인한다. 현재 DRAFT의 픽밴은 결과에 포함하지
않는다. `POST .../revert`는 source entry route로, `DELETE`는 series list로 이동한다. lifecycle은 확정
game도 역순 void/MMR reversal하므로 UI가 오래된 "먼저 게임을 하나씩 되돌려야 함" 안내를 표시하지 않는다.
모든 오류는 backend가 준 safe message/code/request ID를 그대로 표시한다.

WebSocket:

- active operator만 `guild:{guildId}:series:{id}` topic을 구독하고 `invalidate`/`resync_required` 때 active
  snapshot을 다시 읽는다. 다른 운영자의 조기 종료로 active snapshot이 `COMPLETED`가 되면 result GET을
  확인한 뒤 `/result`로 이동한다. 일반 guild member와 completed result 화면은 WebSocket을 열지 않고 HTTP
  read만 사용한다.

### `/g/[guildId]/series/[id]/result`

파일:

- `apps/web/src/routes/g/[guildId]/series/[id]/result/+page.svelte`

목적:

- 일반 내전 최종 결과.
- 게임별 pick/ban, 결과, MMR 변화, 참가자 기록 표시.
- `durationSeconds`가 null이면 `SeriesResultLedgerBoard` adapter가 게임 시간을 `-`로 표시한다.

Backend 데이터:

- `GET /api/guilds/:guildId/series/:id/result`
- `GET /api/guilds/:guildId/series/:id/balance.svg`

완료 series만 렌더링한다. operator에게만 `canRevertLastGame` 또는 `canDelete`가 true이면 확인 dialog와
game/lifecycle mutation control을 제공한다. 마지막 game revert가 성공해 series가 `IN_PROGRESS`로 바뀌면
response `nextHref`를 사용해 `/pick-ban`으로 이동한다.

WebSocket: 읽기 전용 결과 화면에서는 사용하지 않는다.

## 경매 내전 Routes

### `/g/[guildId]/auction`

파일:

- `apps/web/src/routes/g/[guildId]/auction/+page.svelte`

목적:

- 상단에는 경매 모집과 진행 중 경매 토너먼트, 하단에는 완료된 경매를 표시한다.
- 팀장 설정, 입찰 진행, 브래킷 진행, 결과 입력, 결과 확정 상태를 구분한다.
- `IN_GAME` row는 현재 작업을 여는 primary link와 read-only `토너먼트 현황` link를 함께 제공한다.
  진행 현황은 active guild member가 같은 브래킷 화면에서 경기 점수·승자·팀 구성을 확인하는 read-only route다.
- actionable 모집, 진행 중 tournament, 완료/보관 tournament는 각각 독립 `nextCursor`를 가진다. 각
  section의 `더 불러오기`는 해당 cursor만 이어 읽고 ID 기준으로 중복을 제거한다. 진행 중 section은 모집과
  tournament cursor가 모두 남아 있으면 둘을 함께 읽은 뒤 같은 작업 목록으로 병합한다.
- ACTIVE guild member가 10인 또는 20인 형식, 결승 BO1/BO3/BO5, 하드 피어리스를 고르는 작은 경매 모집
  dialog를 제공한다. 20인은 두 준결승에 공통 적용할 4강 BO1/BO3/BO5를 별도로 고른다. 생성 성공 전에는
  목록을 낙관적으로 추가하지 않고, `201` snapshot의 `recruitment.href`로 OPEN 모집 상세 화면에 이동한다.
- 생성 dialog는 Discord actor, title, 정원 또는 setup team 값을 직접 받지 않는다. format과 rules만 전송하고
  target count, 표시 번호, 현재 season, Discord 발행 대상은 backend가 결정한다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-recruitments?state=actionable`
- `GET /api/guilds/:guildId/auction-tournaments?state=active`
- `GET /api/guilds/:guildId/auction-tournaments?state=completed`
- `POST /api/guilds/:guildId/auction-recruitments`

WebSocket: 사용하지 않는다.

### `/g/[guildId]/auction/recruitments/[id]`

파일:

- `apps/web/src/routes/g/[guildId]/auction/recruitments/[id]/+page.svelte`

목적:

- OPEN 경매 모집의 canonical 참가자 roster를 확인한다. Discord 참가와 Web 운영자 수동 참가가 같은 roster를
  갱신한다.
- 모집 header에는 backend snapshot의 규칙을 한 번만 압축해 표시한다. 10인은 `BO3 · 일반`, 20인은
  `4강 BO1 · 결승 BO5 · 하드 피어리스`처럼 표시하며, roster와 목록에 같은 규칙을 반복하지 않는다.
- `permissions.canManageMembers`가 true인 현재 active operator에게만 ACTIVE guild member picker를 제공한다.
  추가할 member와 declared lane을 함께 선택하며, Discord user와 development dummy user는 같은 memberId 경로로
  처리한다.
- backend `permissions`가 허용할 때만 `모집 마감`, `모집 취소` action을 제공한다. 모집 마감의
  정원/lifecycle 조건은 backend가 최종 검증한다.
- 모집 마감 성공 응답의 `links.draft`로 실제 마감 명단 화면에 이동한다. CLOSED 모집은 같은 link를
  바로 제공한다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-recruitments/:id`
- `POST /api/guilds/:guildId/auction-recruitments/:id/members`
- `DELETE /api/guilds/:guildId/auction-recruitments/:id/members/:userId`
- `POST /api/guilds/:guildId/auction-recruitments/:id/close`
- `DELETE /api/guilds/:guildId/auction-recruitments/:id`

WebSocket:

- `permissions.canManageMembers`가 true인 현재 active operator만
  `guild:{guildId}:auction-recruitment:{id}`를 구독한다. `invalidate` 또는 `resync_required` 수신 뒤에는
  authoritative HTTP snapshot을 다시 읽는다.
- 일반 guild member는 socket을 열지 않고 HTTP read만 사용한다. 참가자 roster의 권위는 HTTP snapshot이며,
  mutation 성공 뒤에는 응답 snapshot으로 교체한다.

### `/g/[guildId]/auction/recruitments/[id]/draft`

파일:

- `apps/web/src/routes/g/[guildId]/auction/recruitments/[id]/draft/+page.svelte`

목적:

- CLOSED 경매 모집에서 팀장과 팀별 시작 포인트를 설정하고 입찰 토너먼트를 생성한다.
- OPEN 모집으로 접근하면 모집 상세 route로 되돌린다. 팀 수는 saved format에서 정해지며 10인은 2팀,
  20인은 4팀으로 고정한다.
- 모집 때 확정한 준결승/결승 BO와 하드 피어리스는 header에서 읽기 전용으로 보인다. 이 route에서 format이나
  경기 규칙을 다시 선택하지 않는다.
- `permissions.canEdit`과 `permissions.canCreateTournament`가 허용한 operator만 설정 저장과 tournament
  생성을 실행한다. 저장 및 생성 모두 setup draft revision을 사용한다.

Bot 딥링크:

- CLOSED 모집의 `links.draft` 및 Discord 모집 메시지의 동일 링크에서 이 route로 이동한다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-recruitments/:id`
- `GET /api/guilds/:guildId/auction-recruitments/:id/setup-draft`
- `PATCH /api/guilds/:guildId/auction-recruitments/:id/setup-draft`
- `POST /api/guilds/:guildId/auction-tournaments`

WebSocket: 사용하지 않는다. 저장과 생성 성공 뒤에는 response의 authoritative setup/tournament snapshot으로
교체하거나 응답 `tournament.href`로 이동한다.

### `/g/[guildId]/auction/tournaments/[id]/draft`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/draft/+page.svelte`

목적:

- `BIDDING` 단계의 경매 토너먼트 드래프트.
- 후보 draw, 낙찰, 유찰, 유찰 복구와 마지막 입찰/드래그 배치 단계 되돌리기를 제공한다.
- 현재 매물 기본 화면은 프로필, Riot tier, 모집 시점 MMR, 해당 tournament season의 내전 요약을 보여준다.
  `상세보기`는 라인별 MMR, 확정 내전의 주력 챔피언·최근 게임, 신청 라인의 선호 챔피언, cached Riot 요약을
  기존 정보 영역으로 확장한다. Riot cache가 비어 있으면 해당 영역만 빈 상태로 유지하며 provider refresh는
  발생시키지 않는다. 상세 조회값은 입찰 MMR을 대체하지 않는다.
- 모든 팀 roster가 채워지면 server 상태가 `BRACKET_SETUP`으로 전환된다. Web은 local count로 다음 단계를
  추론하지 않고 다음 snapshot의 status와 permissions를 따른다.
- `BIDDING` 동안에는 브래킷으로 이동하는 control을 노출하지 않는다. 직접
  `/g/[guildId]/auction/tournaments/[id]/bracket`에 접근해도 backend의 lifecycle `409`을 받아 draft로
  되돌린다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id`
- `POST /api/guilds/:guildId/auction-tournaments/:id/commands`

WebSocket: `BIDDING` 상태의 `permissions.canManageDraft=true` active operator만
`guild:{guildId}:auction-tournament:{id}`를 구독한다. command는 HTTP로 확정하며, 더 높은 revision invalidate,
reconnect, `resync_required`에서는 GET snapshot을 다시 적용한다. 일반 guild member와 이후 단계는 socket을
열지 않는다.

### `/g/[guildId]/auction/tournaments/[id]/bracket`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/bracket/+page.svelte`

목적:

- `BRACKET_SETUP` 단계에서 tournament 팀을 slot에 배치하고 bracket을 확정한다.
- 10인은 결승 slot 두 개만, 20인은 두 준결승 slot 네 개를 표시한다. match의 BO는 saved rules에서 읽으며
  화면이 BO3을 기본값으로 만들거나 바꾸지 않는다.
- 10인은 lock 뒤 곧바로 결승 canonical match card를
  `/g/[guildId]/auction/matches/[matchId]`로 연다.
- 20인은 lock 뒤 중앙 준결승 라인업 단계에서 네 팀, 20명의 lane을 함께 편성한다. 두 준결승의
  10인 lineup이 모두 lock되기 전에는 어느 준결승 match의 픽밴·세트 결과 작업대로도 진행하지 않는다.
- 브래킷 route에는 라인업·픽밴·세트 결과 입력 board를 두지 않는다.
- `permissions.canRecoverBracket=true`인 active operator에게만 `대진 확정 해제` 확인 control을 보인다.
  이 command는 결과/픽밴 artifact가 전혀 없을 때만 생성 match를 취소 이력으로 남기고 `BRACKET_SETUP`
  편집 상태로 되돌린다. client는 eligibility를 추정하거나 우회하지 않는다.
- `IN_GAME`, `COMPLETED`, `CANCELLED` 상태로 직접 접근하면 `/result`로 redirect한다. 이 route는
  `BRACKET_SETUP` 편집 단계에만 남긴다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id/bracket`
- `POST /api/guilds/:guildId/auction-tournaments/:id/bracket/commands`

WebSocket: 사용하지 않는다. bracket command 성공과 `409` 충돌 복구는 bracket snapshot을 다시 적용한다.

### `/g/[guildId]/auction/tournaments/[id]/lineup`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/lineup/+page.svelte`

목적:

- lock된 20인 브래킷의 `SEMIFINAL_A`, `SEMIFINAL_B` match snapshot을 함께 읽어 기존 경기 라인업 보드 두 개로
  네 팀, 20명의 lane을 한 단계에서 편성한다.
- 각 lineup mutation은 해당 match의 revision과 독립 idempotency key를 사용한다. 한 준결승의 저장 충돌은
  다른 준결승의 화면 state를 덮어쓰지 않고 해당 match snapshot만 다시 읽는다.
- 두 준결승 lineup이 모두 lock되기 전에는 pick/ban link를 보이지 않는다. 두 match가 모두 준비된 뒤에만
  각각의 canonical match 작업대로 이동한다.
- 10인 tournament나 unlock bracket은 이 route에서 bracket route로 되돌린다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id/bracket`
- `GET /api/guilds/:guildId/auction-matches/:matchId` for `SEMIFINAL_A` and `SEMIFINAL_B`
- 기존 lineup assign/lock/unlock endpoints

WebSocket: 사용하지 않는다. authoritative HTTP snapshot, match revision, CSRF, idempotency key로만 상태를
확정한다.

### `/g/[guildId]/auction/matches/[matchId]`

파일:

- `apps/web/src/routes/g/[guildId]/auction/matches/[matchId]/+page.svelte`

목적:

- 10인 경매에서는 브래킷에서 선택한 결승 match의 라인업 확정과 게임별 pick/ban·승패 입력을 한
  authoritative snapshot으로 렌더링한다.
- 20인 경매에서는 중앙 준결승 라인업 단계가 두 준결승의 lineup을 모두 lock한 뒤에만 각 준결승 match의
  pick/ban·승패 입력 작업대로 진입한다. match route에서 한 준결승만 먼저 편성하거나, 다른 준결승의
  라인업이 남은 상태로 결과 입력을 시작할 수 없다. 이 조건이 남아 있는 준결승 match URL을 직접 열면
  중앙 준결승 라인업 route로 redirect한다.
- operator는 양 팀의 TOP/JUNGLE/MID/BOTTOM/SUPPORT를 각각 한 명씩 배치한 뒤 라인업을 확정한다. 확정 전에는
  결과 입력을 허용하지 않는다.
- 확정 뒤에는 saved `bestOf`, `hardFearless`, 이전 확정 게임, 현재 라인업을 기준으로 다음 세트만 입력한다.
  같은 세트의 pick/ban 중복은 막고, 하드 피어리스는 이전 확정 세트의 pick을 현재 pick으로 다시 선택하지
  못하게 한다. bans 자체는 과거 세트의 피어리스 금지 대상이 아니다.
- 마지막 확정 게임은 capability가 있을 때만 되돌린다. 점수, match 상태, MMR은 client가 계산하지 않고
  response snapshot으로 갱신한다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-matches/:matchId`
- `PATCH /api/guilds/:guildId/auction-matches/:matchId/lineup`
- `POST /api/guilds/:guildId/auction-matches/:matchId/lineup/lock`
- `POST /api/guilds/:guildId/auction-matches/:matchId/lineup/unlock`
- `POST /api/guilds/:guildId/auction-matches/:matchId/games`
- `DELETE /api/guilds/:guildId/auction-matches/:matchId/games/last`

불변 조건:

- lineups locked 전에는 `POST /api/guilds/:guildId/auction-matches/:matchId/games`를 호출할 수 없다.
- 각 팀은 TOP/JUNGLE/MID/BOTTOM/SUPPORT가 정확히 1명씩 있어야 한다.
- 20인 준결승은 두 match의 lineup이 모두 lock되기 전에는 어떤 match도 pick/ban 또는 game result 단계로
  진입할 수 없다.
- `permissions.canUnlockLineup`이 true일 때만 lock된 lineup의 recovery control을 보인다. unlock은 `READY`
  상태이며 games와 game_inputs가 전혀 없는 경우에만 성공하므로 client가 자체 조건으로 override하지 않는다.

WebSocket: 사용하지 않는다. 모든 mutation은 match revision과 idempotency key를 보내며, 충돌 뒤에는 같은
canonical match snapshot을 다시 읽는다.

### `/g/[guildId]/auction/tournaments/[id]/result`

파일:

- `apps/web/src/routes/g/[guildId]/auction/tournaments/[id]/result/+page.svelte`

목적:

- 경매 토너먼트의 read-only 브래킷 현황 및 결과다.
- `AuctionTournamentBracketResultBoard`가 20인은 좌측 준결승 A/B와 우측 결승, 10인은 결승 단일 구조로
  대진을 표시한다. 기본 간략 대진에서 `상세보기`를 선택하면 각 team의 확정 lineup까지 읽기 전용으로
  확인할 수 있다. match score, 승자와 canonical match 진입점은 같은 대진 카드에 둔다.
- 진행 중에는 경매 목록, 브래킷, `READY`/`IN_PROGRESS` match 작업대에서 이 route로 이동한다. 현재 작업을
  바꾸는 control은 각 canonical match route에 남긴다.
- `IN_GAME`이고 result snapshot의 `permissions.canRecoverBracket=true`이면 active operator에게만 `대진 확정
  해제` 확인 control을 보인다. 성공 시 `/bracket`의 편집 단계로 이동한다. 결과가 기록된 이후의 복구는
  canonical match route에서 마지막 game을 먼저 되돌려야 하며, result route가 범용 rollback을 만들지 않는다.

Backend 데이터:

- `GET /api/guilds/:guildId/auction-tournaments/:id/result`
- `POST /api/guilds/:guildId/auction-tournaments/:id/bracket/commands` (`UNLOCK_BRACKET`, capability가 true일 때만)

WebSocket은 사용하지 않는다.

## 사용자 / 통계 Routes

### `/g/[guildId]/records`

파일:

- `apps/web/src/routes/g/[guildId]/records/+page.svelte`

목적:

- 현재 guild의 내부 내전 전적 검색 진입점.
- Discord 사용자, 연결 Riot ID, guild 표시 이름으로 사용자를 찾는다.

Backend 데이터:

- `GET /api/guilds/:guildId/records/search?q={exactIdentity}&seasonId={uuid}&cursor={opaque}&limit=20`
- `GET /api/guilds/:guildId/records/profile?q={exactIdentity}`는 identity-only 호환 projection이다.

`search`는 Discord ID, guild display name 또는 공개가 허용된 정확한 Riot ID로 active member 한 명을
찾고 identity, season, MMR, champion aggregate와 일반/경매 기록을 한 snapshot으로 반환한다. 응답의
`member`와 전역 `championPreferences`는 항상 표시하고, `riotAccounts`는 backend가 `show_riot_ids`를
적용한 결과만 사용한다. 빈 배열이면 Riot ID 행을 만들지 않으며 query에서 `#KR1`을 합성하지 않는다.
계정이 여러 개 공개되면 main 계정을 먼저 표시하되 모두 확인할 수 있다.

선호 챔피언은 포지션별 사용자 지정 순서이며 season별 `챔피언 사용 기록`과 별도 영역으로 표시한다.
검색 결과 헤더의 사용자명은 `member.profileHref`가 있을 때만 `/users/[discordId]`로 연결한다. Frontend는
search/profile 응답에서 Discord user ID나 PUUID를 기대하거나 privacy를 재판단하지 않고, nullable canonical
href가 `null`이면 일반 텍스트로 표시한다.

Season control은 응답의 실제 `seasons`만 사용한다. 초기 요청에 `seasonId`가 없으면 backend의 ACTIVE
selection을 사용하고, 사용자가 season을 바꾸면 cursor를 비운 새 server navigation을 수행한다. ACTIVE
season이 없을 때 종료 season을 자동 선택하거나 `전체 시즌`을 합성하지 않는다. `selectedSeasonId=null`이면
season 통계와 기록의 empty state를 표시한다. 존재하지 않거나 다른 guild의 seasonId는 `404`다.

라인 MMR은 canonical 다섯 lane을 그대로 렌더링한다. 아직 row가 없는 lane도 backend가 1500/0게임/0승으로
projection하므로 client가 MMR이나 change를 추론하지 않는다. Champion 사용 기록은 champion+lane별 row다.
일반/경매, lane 버튼은 현재 읽은 `records` page에 대한 local filter이며 query parameter를 추가하지 않는다.
`더 보기`는 현재 `q`와 season selection을 유지하고 `nextCursor`만 보내 append/dedupe한다.

Outer `SERIES`는 일반과 10인 경매를 기존 series row/expand UI로 렌더링한다. `TOURNAMENT`는 20인 경매
한 건이며 내부 series selector와 game detail을 유지한다. API timestamp와 `WIN|LOSS|DRAW`,
`SEMIFINAL_A|SEMIFINAL_B|FINAL`, numeric wins/losses/MMR로 상대 시간, 한국어 label, score를 만든다.
Nullable champion/MMR에는 깨진 이미지나 가짜 수치를 만들지 않고 없음 상태를 표시한다.

주의:

- 현재 `profile`과 `search` endpoint는 same-guild active member 전용이며 외부 요청에는 `404`를 반환한다.
  `allowExternalRecordSearch`는 이후 guild 외부 custom match 공개 경로에 적용할 별도 범위다.
- 내부 내전 기록은 반드시 현재 guild의 `games` 원장만 조회한다.
- `guild_config.is_externally_public`만으로 이 same-guild route의 권한을 넓히지 않는다. 외부 공개 route는
  `allowExternalRecordSearch`와 함께 별도 계약이 정의될 때 추가한다.

### `/records`

파일:

- `apps/web/src/routes/records/+page.svelte`

목적:

- navbar Riot ID 검색 결과를 표시한다.
- Riot profile/rank/champion aggregate/recent match와 10인 상세를 보여준다.
- guild 내부 custom match 원장과 분리한다.

Backend 데이터:

- `GET /api/riot/records?gameName={name}&tagLine={tag}&platform=KR`
- `POST /api/riot/records/refresh` with `{ gameName, tagLine, platform }`

Navbar/UI의 `GameName#TagLine`은 frontend adapter에서 마지막 `#` 기준으로 두 query field로 나눈다.
Browser는 같은 origin의 `/api/**`만 호출하고 Riot key, PUUID, regional routing을 만들지 않는다.

WebSocket: 사용하지 않는다.

### `/users/[discordId]`

파일:

- `apps/web/src/routes/users/[discordId]/+page.svelte`

목적:

- 사용자 프로필.
- 현재 MVP에서는 표시 이름, Discord ID, 활동 guild 수, season 수를 보여준다.
- `/g/[guildId]/records` 검색 결과 헤더의 공개 사용자명에서 진입한다.

Backend 데이터:

- `GET /api/users/:id/profile`

WebSocket: 사용하지 않는다.

### `/g/[guildId]/leaderboard`

파일:

- `apps/web/src/routes/g/[guildId]/leaderboard/+page.svelte`

목적:

- ACTIVE season의 길드 MMR을 라인별/통합 리더보드로 표시한다.

Backend 데이터:

- `GET /api/guilds/:guildId/leaderboard`

`+page.server.ts`가 인증 cookie를 전달해 snapshot을 한 번 읽는다. 활성 season이 없으면
`season=null, players=[]`을 정상 empty state로 표시한다. API의 `stats`는 실제 game이 한 건 이상인 lane만
포함하며, Riot ID와 profile icon은 backend가 적용한 `show_riot_ids` projection을 그대로 신뢰한다.

통합/라인 탭, player 검색, 25/50/100 표시 제한, MMR 정렬, 순위와 승률은 읽은 전체 snapshot 위에서
`LeaderboardBoard`가 local state로 처리한다. 이 UI state를 query parameter나 별도 API로 보내지 않는다.

WebSocket: 사용하지 않는다.

### `/me`

파일:

- `apps/web/src/routes/me/+page.svelte`

목적:

- 선호 챔피언, Riot 계정, 대표 길드, 개인정보 설정을 하나의 내 정보 화면에서 관리한다.
- 개인정보 설정은 외부 프로필, Discord 사용자와 연결된 Riot ID 관계, 길드 밖의 custom match 전적
  접근만 제어한다. public Riot ID/외부 Riot 전적 검색은 개인정보 설정으로 제한하지 않는다.
- 같은 guild 내부의 내전 기록과 MMR은 개인 설정으로 숨기지 않는다.

Backend 데이터:

- `GET /api/me/champion-preferences`
- `PUT /api/me/champion-preferences/:lane`
- `GET /api/me/guilds`
- `PUT /api/me/default-guild`
- `GET /api/me/riot-accounts`
- `POST /api/me/riot-accounts`
- `DELETE /api/me/riot-accounts/:puuid`
- `PUT /api/me/riot-accounts/:puuid/main`
- `POST /api/me/riot-accounts/:puuid/refresh`
- `GET /api/me/privacy`
- `PUT /api/me/privacy`

챔피언 선호 GET/PUT 응답은 항상
`{ preferences: { TOP, JUNGLE, MID, BOTTOM, SUPPORT } }` 다섯 key를 포함한다. 각 값은 사용자 지정 순서의
중복 없는 champion ID 배열이며 최대 10개다. PUT은 `{ championIds }`로 선택 lane 전체를 교체하고, UI는
응답의 full snapshot으로 saved/draft state를 갱신한다.

계정 추가는 `{ gameName, tagLine, platform }`만 보내며 backend가 PUUID와 canonical identity를 확인한다.
Riot account mutation은 모두 최신 `{ riotAccounts: [...] }` snapshot을 반환하므로 mutation 직후 별도 GET이나
client-side main account 추론을 하지 않는다.

WebSocket: 사용하지 않는다.

## 도구 Routes

### `/docs`

파일:

- `apps/web/src/routes/docs/+page.svelte`
- `apps/web/src/lib/content/user-guide.md`

목적:

- 일반 사용자와 길드 운영자를 위한 정적 기능 설명.
- Markdown 원본을 build 시 렌더링하며 API나 사용자 상태를 요구하지 않는다.

### `/terms`

파일:

- `apps/web/src/routes/terms/+page.svelte`
- `apps/web/src/lib/content/terms.md`

목적:

- Discord Bot과 웹 콘솔 이용 조건을 공개하는 정적 이용약관.
- build 시 Markdown을 렌더링하고 Footer에서 접근한다.

### `/privacy`

파일:

- `apps/web/src/routes/privacy/+page.svelte`
- `apps/web/src/lib/content/privacy.md`

목적:

- 현재 구현된 Discord 인증, 길드 운영 데이터, 세션과 선택적 Riot 연동의 처리 범위를 공개한다.
- build 시 Markdown을 렌더링하고 Footer에서 접근한다.

`/docs`, `/terms`, `/privacy`는 `StaticMarkdownDocument`를 공유하며 API나 로그인 상태를
요구하지 않는다. 세 route에서는 운영용 `QuickActionFab`을 표시하지 않는다.

미니게임과 타이머는 독립 route를 만들지 않고 전역 `QuickActionFab` modal로만 제공한다.

## Admin Routes

### `/g/[guildId]/admin`

파일:

- `apps/web/src/routes/g/[guildId]/admin/+layout.svelte`
- `apps/web/src/routes/g/[guildId]/admin/+layout.server.ts`
- `apps/web/src/routes/g/[guildId]/admin/+page.svelte`

목적:

- 길드 멤버 등록 상태, Riot 연결 상태, MMR을 조회하고 관리한다.
- 운영자 역할, 모집/엔트리/로그 채널과 외부 공개 설정을 관리한다.
- audit log를 필터링하고 destructive/admin action을 추적한다.
- checkpoint/revert 운영 탭은 MVP에서 노출하지 않는다.

탭:

- `members`: 멤버 검색, MMR 조정, 비활성화와 복구.
- `settings`: Discord 역할/채널과 공개 설정.
- `seasons`: `/g/[guildId]/admin/seasons`의 별도 route로 이동한다.
- `logs`: 작업, 대상, 실행자별 감사 로그와 상세 payload.

관리 헤더와 OPERATOR 권한 검사는 admin layout이 공통으로 제공한다. `settings` tab의 server load는
실제 settings/catalog snapshot을 조회한다. 모집 채널은 모집 메시지의 필수 목적지다. 엔트리 채널은 밸런스
결과와 확정 엔트리, 결과 채널은 일반 시리즈 최종 결과, 로그 채널은 운영 알림의 목적지를 각각 표현하며
후자 세 channel만 `설정 안 함`을 지원한다. legacy `recruitmentChannelId=null` snapshot은 읽을 수 있지만
다음 저장 전에 채널을 선택해 복구해야 한다.

`members` tab의 server load는 다음 strict snapshot을 읽는다.

```text
revision
updatedAt
updatedBy | null
members[]
  id
  discordName
  riotId | null
  role                 OPERATOR | MEMBER
  mainLane | null
  laneMmr              5개 canonical lane 전체
  registered
  status               ACTIVE | INACTIVE
  lastActiveAt | null
```

`mainLane`은 미등록/미설정 member에서 `null`일 수 있다. `GuildMemberManagementPanel`은 목록에서 임의
lane을 합성하지 않고 `미설정`으로 표시하며, MMR modal에서 사용자가 lane을 명시적으로 선택하기 전에는
저장할 수 없다. `lastActiveAt`은 timestamp를 client에서 상대 시간으로 표시하고 null은 `-`로 표시한다.
`role`의 한국어 label도 client adapter가 만든다.

현재 로그인 사용자의 row에서는 비활성화 action을 disabled로 표시한다. Backend도 actor 자신의
`ACTIVE -> INACTIVE` 요청을 `409 self_deactivation_forbidden`으로 거부하므로 UI 제약을 우회해도 admin
접근 권한을 잃지 않는다. 길드 OWNER 개념은 없으며 현재 Discord operator role과 ACTIVE member 상태만
권한 근거로 사용한다.

`settings` tab은 다음 strict snapshot을 읽는다.

```text
settings
  displayName
  operatorRoleId
  recruitmentChannelId | null
  entryChannelId | null
  resultChannelId | null
  logChannelId | null
  isExternallyPublic
revision
updatedAt
updatedBy | null
catalogUpdatedAt | null
roles[]                 id, name
channels[]              id, name, parentName | null
```

`GuildSettingsAdminPanel`은 role/channel fixture를 import하지 않는다. Catalog에 없는 현재 설정 ID는 첫
option으로 임의 변경하지 않고 `Discord에서 확인 불가` fallback으로 유지한다. 저장 중에는 모든 입력을
잠그고, 성공 응답의 settings와 revision으로 saved/draft를 함께 교체한다.

`logs` tab의 server load는 `GET /api/guilds/:guildId/audit-logs?limit=50`의 다음 strict page를 읽는다.

```text
entries[]
  id
  principalType
  principalId
  principalDisplayName | null
  action
  targetType | null
  targetId | null
  targetDisplayName | null
  checkpointId | null
  requestId | null
  beforeSummary | null
  afterSummary | null
  result
  note | null
  createdAt
nextCursor | null
```

`GuildAuditLogPanel`은 fixture를 import하지 않는다. Canonical action/result를 한국어 label과 category로
변환하되 알 수 없는 action은 raw code와 `기타`로 남긴다. 표시명은 API가 제공한 현재 projection을 쓰고,
member, guild/config, season, recruitment 외의 null target은 target type/ID로 fallback한다. Null principal은
principal type/ID로 fallback한다. 펼침 상세는 저장된 note와 before/after summary만 표시하며 mock 설명을
합성하지 않는다. Timestamp는 client에서 상대 시간으로 표현한다.

검색, category, 실행자 필터는 현재까지 읽은 entry에 local로 적용한다. `nextCursor`가 있으면 `더 보기`가
같은 endpoint를 cursor와 함께 호출해 ID 기준으로 중복 제거 후 누적한다. 요청 중 버튼을 잠그고 실패 시
기존 entry를 유지한다. 감사 로그는 append-only 조회 화면이므로 WebSocket, CSRF, revision과 idempotency
key를 사용하지 않는다.

### `/g/[guildId]/admin/seasons`

파일:

- `apps/web/src/routes/g/[guildId]/admin/seasons/+page.server.ts`
- `apps/web/src/routes/g/[guildId]/admin/seasons/+page.svelte`
- `apps/web/src/lib/api/guildAdminSeasons.ts`
- `apps/web/src/lib/contracts/workflow.ts`

목적:

- 현재 시즌의 이름, 기간, 생성자와 참가자/일반 시리즈/경매 내전/확정 게임 집계를 확인한다.
- 종료된 시즌을 펼쳐 같은 집계와 10인/20인 경매 구성, 최근 확정 경기를 확인한다.
- `새 시즌 시작` modal은 이름 하나만 입력받고 현재 시즌 ID는 화면 snapshot에서 전송한다.
- Server load는 `GET /api/guilds/:guildId/seasons`의 `{current, completed}` snapshot을 사용한다.
- 전환 성공 시 `POST /api/guilds/:guildId/seasons` 응답의 authoritative snapshot으로 현재/종료 목록을
  함께 교체한다.
- 시작일/종료일, 상태, MMR 초기값과 기록 초기화 옵션은 Web 입력으로 만들지 않는다.

Season summary는 `id`, `name`, `status`, `startedAt`, `endedAt`, `createdBy`, 참가자/일반 시리즈/경매
내전/확정 게임 수, 경매 10인/20인 수와 `lastGameAt`을 포함한다. API timestamp는 client에서 날짜와
기간으로 표시하고 null 종료일/최근 경기/생성자는 각각 진행 중/없음/`시스템`으로 표시한다.

Admin member endpoint는 OPERATOR 전용 운영 projection이므로 `riotId`는 `showRiotIds`와 무관한 main
Riot ID이고 null은 미연결만 의미한다. Public/guild records 화면의 privacy-filtered Riot projection과
혼합하지 않는다.

호환 경로:

- `/g/[guildId]/admin/logs`는 `/g/[guildId]/admin?tab=logs`로 redirect한다.
- `/g/[guildId]/admin?tab=operations`는 MVP에서 `/g/[guildId]/admin?tab=members`로 redirect한다.

현재 연동 데이터:

- `GET /api/guilds/:guildId/members`
- `PATCH /api/guilds/:guildId/members/:memberId/mmr`
- `PATCH /api/guilds/:guildId/members/:memberId/status`
- `GET /api/guilds/:guildId/settings`
- `PUT /api/guilds/:guildId/settings`
- `GET /api/guilds/:guildId/seasons`
- `POST /api/guilds/:guildId/seasons`
- `GET /api/guilds/:guildId/audit-logs`

시즌 API는 settings payload와 revision에 포함하지 않는다. 감사 로그 page도 settings/members/seasons
snapshot과 분리해 active tab에 필요한 endpoint만 server load한다.

권한:

- 현재 guild operator.

두 member mutation은 CSRF와 `expectedRevision`을 전송한다. MMR mutation만 논리적인 save intent마다
`crypto.randomUUID()`로 만든 UUID `Idempotency-Key`를 추가하고, 동일 request의 transport 재시도에는
같은 key를 유지한다. Status mutation에는 idempotency key를 보내지 않는다.
두 mutation 응답은 변경 member를 client에서 계산한 결과가 아니라 GET과 동일한 authoritative full
snapshot이다. 성공하면 snapshot 전체를 교체하고, `revision_conflict` 409이면 GET을 다시 호출해 최신
revision과 member 목록으로 교체한 뒤 재시도 안내를 표시한다. 다른 409는 해당 오류를 그대로 표시한다.
자기 비활성화는 button을 미리 disabled로 막고 backend의 `self_deactivation_forbidden`을 일반 revision
충돌로 처리하지 않는다.
이 화면은 WebSocket을 연결하지 않는다.

Settings PUT은 여섯 설정 field와 `expectedRevision`만 전송하며 CSRF가 필수다. 성공하면 backend의 full
settings/catalog snapshot으로 교체한다. `revision_conflict` 409만 GET을 다시 호출해 최신 설정과 선택지를
적용하고, 다른 409/422는 backend 오류를 그대로 표시한다. Settings도 WebSocket과 idempotency key를
사용하지 않는다.

Season POST는 trim한 `name`, snapshot의 `current.id`를 `expectedCurrentSeasonId`로 보내고 CSRF와 논리적
시도마다 한 번 만든 Browser `Idempotency-Key`를 사용한다. 동일 전송을 재시도할 때는 key를 유지한다.
성공 전 local 목록을 옮기지 않으며 성공 응답 전체로 교체한다. `current_season_conflict` 409이면 GET을
다시 읽어 다른 운영자가 전환한 현재 시즌을 표시하고 modal 입력을 유지한다. Seasons 화면도 WebSocket을
연결하지 않는다.

### `/admin/status`

파일:

- `apps/web/src/routes/admin/status/+page.svelte`

목적:

- API, PostgreSQL, Bot heartbeat, Valkey 상태 확인.
- 최초 mount와 새로고침 action에서 authoritative snapshot을 읽는다. `401`, `403`, network, invalid response는
  backend의 message/request ID를 유지한 retryable error panel로 보이며, client가 status를 추정하거나
  mock timestamp를 만들지 않는다.

Backend 데이터:

- `GET /api/admin/status`

권한:

- service admin allowlist.

WebSocket: 사용하지 않는다. Bot heartbeat의 90초/5분 분류와 모든 service 상태는 backend snapshot이
결정한다.

## Frontend 우선순위

### P0

- `/`
- `/oauth/[...path]`
- `/api/[...path]`
- `/dd/[...path]`
- `/g/[guildId]`
- `/g/[guildId]/recruitments/[id]`
- `/g/[guildId]/recruitments/[id]/entry`
- `/g/[guildId]/series/[id]/pick-ban`
- `/g/[guildId]/series/[id]/result`

### P1

- `/g/[guildId]/series`
- `/g/[guildId]/auction`
- `/g/[guildId]/auction/recruitments/[id]`
- `/g/[guildId]/auction/recruitments/[id]/draft`
- `/g/[guildId]/auction/tournaments/[id]/draft`
- `/g/[guildId]/auction/tournaments/[id]/bracket`
- `/g/[guildId]/auction/matches/[matchId]`
- `/g/[guildId]/auction/tournaments/[id]/result`
- `/users/[discordId]`
- `/g/[guildId]/leaderboard`
- `/me`

### P2

- `/g/[guildId]/admin`
- `/admin/status`

## 만들지 않을 Frontend Route

- `/activity/*`: Discord Activity route는 만들지 않는다.
- `/login`: `/oauth/discord/start` 링크 또는 button으로 충분하면 별도 로그인 페이지를 만들지 않는다. 만들더라도 Discord OAuth2 시작 버튼만 둔다.
- `/signup`: 별도 회원가입은 절대 만들지 않는다.
- `/register`: 앱 회원가입 route는 만들지 않는다. Riot ID 연결은 `/me` 또는 Discord `/등록`에서 처리한다.
- `/password/*`: password reset/change flow는 만들지 않는다.
- `/settings`: 전역 통합 설정 route는 만들지 않는다. 사용자 설정은 `/me`, 길드 설정은
  `/g/[guildId]/admin?tab=settings`에서 처리한다.
- `/reports`: 통계 report 제품 요구가 생기기 전까지 만들지 않는다.
- `/users`: 검색은 `/g/[guildId]/records`에서 처리하고 사용자 상세만 `/users/[discordId]`로 연다.
- `/admin/perms`: 권한 실패는 해당 guild route의 403 화면에서 처리한다.
- `/tools/minigame`: 미니게임과 타이머는 `QuickActionFab` modal로 제공한다.
