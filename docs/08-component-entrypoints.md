# MonkeyBot Component Entrypoints

작성일: 2026-07-11

## 목적

이 문서는 이미 만든 UI를 route에서 다시 조립할 때 사용하는 컴포넌트 진입점의 단일 기준이다.

- 어떤 컴포넌트를 직접 import해야 하는지 확인한다.
- 컴포넌트가 배치된 실제 업무 route와 데이터 상태를 확인한다.
- 필요한 props와 데이터 출처를 확인한다.
- 상위 컴포넌트 대신 내부 조각을 잘못 호출하는 일을 막는다.

컴포넌트의 화면 책임은 이 문서, route의 업무 책임은 [Frontend Routing](routing/frontend.md), 동시 편집 계약은 [Realtime Collaboration](07-realtime-collaboration.md)을 따른다.

## 상태 표기

| 상태      | 의미                                               |
| --------- | -------------------------------------------------- |
| Routed    | 실제 운영 흐름의 route에서 직접 렌더링 중          |
| API data  | Backend authoritative snapshot과 mutation을 사용함 |
| Mock data | route 배치는 완료했지만 API 대신 fixture를 사용함  |
| Internal  | 상위 컴포넌트 안에서만 사용하는 구현 조각          |

현재 상위 workflow 컴포넌트는 모두 운영 route에 배치되어 있다. Route별로 API 연결 시점이 다르므로 route
배치 완료, Backend 연동 완료, fixture-only 상태를 구분한다.

## Import 원칙

공용 barrel export는 아직 두지 않는다. 파일을 직접 import한다.

```svelte
<script lang="ts">
	import EntryDraftBoard from '$lib/components/workflow/EntryDraftBoard.svelte';
</script>
```

디렉터리 책임:

```txt
$lib/components/layout      App shell과 화면 조립 primitive
$lib/components/workflow    모집, 편성, 픽밴, 경매, 결과, 사용자 workflow
$lib/components/dev         service-admin 전용 더미 사용자와 기능 배치 UI
$lib/components/fab         전역 빠른 실행과 그 내부 modal
$lib/components/champion    Data Dragon 기반 챔피언 표시 primitive
$lib/fixtures               UI 확인용 목업과 임시 타입
$lib/datadragon             챔피언 catalog loader와 이미지 URL
```

## Layout Entrypoints

| 컴포넌트          | 직접 사용 위치               | 주요 계약                                                                                       |
| ----------------- | ---------------------------- | ----------------------------------------------------------------------------------------------- |
| `AppShell`        | 루트 `+layout.svelte`        | `children`; navbar, 계정 메뉴, 전체 content 폭을 소유한다. 다른 route에서 중복 호출하지 않는다. |
| `WorkspaceHeader` | 각 업무 route 상단           | `kicker`, `title`, 선택 `description`, `metrics`, `actions` snippet                             |
| `Panel`           | route 내부 구획              | `children`, 선택 `kicker`, `title`, `description`, `actions`, `compact`                         |
| `StatusPill`      | 표와 작업 상태               | `label`, 선택 `tone`                                                                            |
| `RowList`         | 동일 column 구조의 반복 목록 | `children`, 선택 `columns`, `headings`                                                          |
| `ChampionFace`    | 챔피언 portrait가 필요한 행  | `champion`, `version`, 선택 `size`                                                              |
| `PlayerAvatar`    | 참가자 avatar가 필요한 행    | `name`, `role`; 선택 `avatarUrl`, `profileIconId`, `profileIconVersion`, `size`, `shape`        |
| `PositionIcon`    | 라인/포지션 표시             | `role`; 선택 `size`                                                                             |

`WorkspaceHeader`는 route의 H1을, `Panel`은 route 내부 H2 수준 구획을 담당한다. 업무 보드를 `Panel` 안에 다시 카드 형태로 감싸는 중첩은 만들지 않는다.

## 일반 내전 Entrypoints

| 컴포넌트                           | 운영 route                                         | 필수 입력                                                                                                 | 데이터 상태  |
| ---------------------------------- | -------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------ |
| `EntryDraftBoard`                  | `/g/[guildId]/recruitments/[id]/entry` 편성 탭     | `participants`, `initialAssignments`; 선택 `synergies`, `headToHead`, `targetCount`, `profileIconVersion` | Mock data    |
| `SeriesCompletedEntryRosterBoard`  | `/g/[guildId]/recruitments/[id]/entry` 미리보기 탭 | `participants`, `assignments`, `champions`; 제목, 팀 이름, 포맷은 선택                                    | Mock data    |
| `ScrimPickBanResultInputBoard`     | `/g/[guildId]/series/[id]/pick-ban` 입력 탭        | `teams`, `initialGame`, `champions`, `patchVersion`, `onSubmit`                                           | Mock adapter |
| `ScrimPickBanResultSetSummaryCard` | `/g/[guildId]/series/[id]/pick-ban` 요약 탭        | `label`, 양 팀, `winner`, `bans`, `picks`, `champions`, `version`                                         | Mock data    |
| `SeriesResultLedgerBoard`          | `/g/[guildId]/series/[id]/result`                  | `catalog: ChampionCatalog`; 선택 `seriesId`                                                               | Mock data    |

`/g/[guildId]/series`는 상단에 모집 마감, 엔트리 편성, 픽밴, 결과 입력 상태를 보여주고 하단에 완료 시리즈를 보여준다. 편성, 픽밴, 결과 컴포넌트를 목록 route에 직접 쌓지 않는다.

## 경매 Entrypoints

`/g/[guildId]/auction`은 상단에 경매 모집과 진행 중 토너먼트, 하단에 완료된 경매를 보여준다. 각 컴포넌트는 아래 단계 route에서만 렌더링한다.

| 컴포넌트                                  | 운영 route                                                  | 필수 입력                                                                                      |
| ----------------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `AuctionTournamentCaptainPointSetupBoard` | `/g/[guildId]/auction/recruitments/[id]/draft`              | `initialMembers`; 선택 `initialCaptainIds`, `teamCount`, `initialPoints`, `profileIconVersion` |
| `AuctionTournamentBidDraftWorkbench`      | `/g/[guildId]/auction/tournaments/[id]/draft`               | `initialTeams`, `initialQueue`, `revision`, `onCommand`; 선택 `profileIconVersion`             |
| `AuctionTournamentBracketMatchSetupBoard` | `/g/[guildId]/auction/tournaments/[id]/bracket`             | `teams`, `snapshot`, `onCommand`; 선택 `profileIconVersion`                                    |
| `AuctionMatchStartingLineupInputBoard`    | `/g/[guildId]/auction/tournaments/[id]/bracket`             | `leftTeam`, `rightTeam`; 선택 `profileIconVersion`                                             |
| `AuctionTournamentBracketResultBoard`     | `/g/[guildId]/auction/tournaments/[id]/bracket?view=result` | `teams`, `matches`, `onSubmitResult`; 선택 `profileIconVersion`                                |
| `AuctionTournamentOverviewBoard`          | `/g/[guildId]/auction/tournaments/[id]/result`              | `teams`, `matches`, `championTeamId`; 선택 `profileIconVersion`                                |

경매 mutation 컴포넌트는 작은 command와 `expectedRevision`을 parent adapter로 전달한다. route의 mock adapter는 local snapshot을 갱신하며, API 연결 시 같은 callback을 HTTP command와 최신 snapshot 반영으로 교체한다.

## 사용자와 운영 Entrypoints

| 컴포넌트                        | 운영 위치                                                    | 주요 계약과 상태                                                                 |
| ------------------------------- | ------------------------------------------------------------ | -------------------------------------------------------------------------------- |
| `LeaderboardBoard`              | `/g/[guildId]/leaderboard`                                   | `catalog`, API `snapshot`, 선택 `loadError`; 탭·검색·표시 제한은 local state     |
| `RiotAccountManager`            | `/me` Riot 계정 탭                                           | `initialAccounts`; Riot ID 연결과 snapshot mutation은 `/api/me/riot-accounts/**` |
| `UserChampionPreferencesEditor` | `/me` 선호 챔피언 탭                                         | `catalog`, 선택 `maxPreferences`; 선호 목록과 저장은 내부 목업                   |
| `UserPrivacySettingsPanel`      | `/me` 개인정보 탭                                            | 외부 프로필, Discord↔Riot 연결 관계, 길드 밖 내전 전적 접근 설정                 |
| `UserDefaultGuildSettingsPanel` | `/me` 대표 길드 탭                                           | `initialGuildId`, `onSave`; 루트 접속 시 이동할 기본 guild 선택                  |
| `OperationsQueueBoard`          | `/g/[guildId]`                                               | `items: OperationsQueueItem[]`; 작업 링크는 같은 guild의 운영 route를 가리킨다.  |
| `RealtimeEditStateBar`          | 편성, 픽밴, 경매 모집 설정, 경매 드래프트, 경매 브래킷 route | resource, 연결/저장 상태, 편집자, 충돌 처리 callback                             |
| `QuickActionFab`                | `AppShell` 전역                                              | 선택 `label`; 미니게임과 타이머 modal을 소유                                     |
| `StaticMarkdownDocument`        | `/docs`, `/terms`, `/privacy`                                | 정적 Markdown 본문, 목차, 문서 메타데이터                                        |
| `AppFooter`                     | `AppShell` 전역                                              | 공통 브랜드 Footer와 문서·이용약관·개인정보 처리방침 진입 링크                   |
| `GuildMemberManagementPanel`    | `/g/[guildId]/admin` 멤버 탭                                 | API member snapshot, revision, 자기 비활성화 방지, MMR·상태 command callback     |
| `GuildOperationsAdminPanel`     | MVP 미노출, 후속 롤백 기능 검토용                            | checkpoint snapshot, revision, 고정 revert command                               |
| `GuildSettingsAdminPanel`       | `/g/[guildId]/admin` 길드 설정 탭                            | API settings/catalog snapshot, revision, authoritative 저장 callback             |
| `GuildAuditLogPanel`            | `/g/[guildId]/admin` 감사 로그 탭                            | API cursor page, local 작업·대상·실행자 필터, raw audit 상세, `더 보기`          |

## Development Entrypoints

| 컴포넌트                           | 운영 위치                                 | 주요 계약과 상태                                                                                    |
| ---------------------------------- | ----------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `DummyUserWorkspace`               | `/dev/users`                              | `initialUsers`, `initialGroups`, `initialTotal`, `guilds`, `championCatalog`; 선택 `initialGuildId` |
| `DummyMemberPicker`                | 더미 group 편집과 기능별 더미 참가자 선택 | `users`, `selectedIds`, `onSelectionChange`; 선택 `requiredGuildId`, `maxSelected`, `disabled`      |
| `RecruitmentDummyParticipantPanel` | `/g/[guildId]/recruitments/[id]/entry`    | `guildId`, `recruitmentId`; API overview/preview/apply를 내부에서 연결                              |

세 컴포넌트는 `MONKEYBOT_DUMMY_USERS_ENABLED=true`인 non-production 환경의 service admin 도구다.
`RecruitmentDummyParticipantPanel`은 service admin이면서 현재 길드 operator일 때만 route에서 렌더링한다.
`DummyMemberPicker.requiredGuildId`가 있으면 해당 길드의 ACTIVE membership만 선택하며, 최종 자격·정원·중복
검증은 apply API가 다시 수행한다.

`DummyUserWorkspace`는 persistent DB entity를 관리한다. `$lib/fixtures`를 읽거나 더미 사용자를 browser
memory에만 생성하지 않는다. 일괄 생성은 포함된 길드 배치/MMR까지 한 bulk request로 처리하고, group 연결은
별도 revision mutation 결과를 명시적으로 반영한다. Delete 전에 deletion-impact를 읽고 blocking reference가
있으면 archive를 안내한다.

## Page-local UI

아래 화면은 아직 공유 workflow 컴포넌트 진입점으로 분리하지 않았다.

- `/records`: Riot 전적 검색과 상세 경기 표가 page-local이다.
- `/g/[guildId]/admin/seasons`: 현재/종료 시즌 집계, 종료 시즌 확장 상세와 새 시즌 시작 UI가
  page-local이다. Server load는 `GET /api/guilds/{guildId}/seasons`를 사용하고 새 시즌 시작은
  `POST /api/guilds/{guildId}/seasons`의 authoritative snapshot으로 화면을 교체한다.
- `/g/[guildId]/records`: 실제 guild records snapshot, 내전 시리즈 목록과 확장 상세가 page-local이다.
- `/admin/status`: 운영 상태 UI가 page-local이다.

다른 route에서 같은 업무 UI가 실제로 필요해질 때만 상위 컴포넌트로 추출한다.

`GuildSettingsAdminPanel`은 `$lib/fixtures`를 사용하지 않는다. Settings server load가
`GET /api/guilds/{guildId}/settings`의 설정, revision, Discord role/channel catalog를 전달하고, PUT 성공
응답의 full snapshot으로 draft를 다시 맞춘다. Nullable channel은 `설정 안 함`으로 표시하며 catalog에서
사라진 현재 ID도 fallback option으로 보존한다. 별도 page-local 시즌 UI와 seasons API 계약은 settings
컴포넌트 계약에 포함하지 않는다.

`GuildAuditLogPanel`도 `$lib/fixtures`를 사용하지 않는다. Logs tab server load가
`GET /api/guilds/{guildId}/audit-logs?limit=50`의 첫 page를 전달하고, `더 보기` callback은 opaque cursor로
다음 page를 읽어 누적한다. Component는 canonical action/result presentation과 누적 row 대상 local filter만
소유하며 감사 원장이나 역사적 표시명을 추론하지 않는다.

`/g/[guildId]/records`도 page-local fixture를 사용하지 않는다. Server load는
`GET /api/guilds/{guildId}/records/search?q={exactIdentity}&limit=20`의 identity, season summary, canonical
다섯 lane MMR, champion+lane aggregate와 `SERIES|TOURNAMENT` union을 전달한다. Season 변경은 새 server
selection이고 type/lane은 현재 읽은 page의 local filter다. `더 보기`는 같은 q/season과 opaque cursor로
outer group을 append하며, 20인 tournament의 nested series를 별도 outer row로 분리하지 않는다.

## Data Dragon 사용

챔피언 이미지가 필요한 route는 `+page.ts`에서 catalog를 로드한 뒤 컴포넌트에 전달한다.

```ts
import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';

export async function load({ fetch }) {
	return {
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		})
	};
}
```

```svelte
<script lang="ts">
	import LeaderboardBoard from '$lib/components/workflow/LeaderboardBoard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<LeaderboardBoard
	catalog={data.championCatalog}
	snapshot={data.snapshot}
	loadError={data.loadError}
/>
```

리더보드 `+page.server.ts`는 `GET /api/guilds/{guildId}/leaderboard`를 호출해 `snapshot`을 제공한다. 활성
season이 없는 `season=null, players=[]`은 오류가 아니며, 컴포넌트는 내부 fixture나 별도 WS를 사용하지
않는다.

`profileIconVersion`과 `version`에는 catalog의 `version`을 전달한다. route 컴포넌트에서 Data Dragon URL을 직접 조합하지 않는다.

## RealtimeEditStateBar 사용

동시 편집 화면은 자체 상태 문구를 새로 만들지 않고 이 컴포넌트를 사용한다.

```svelte
<RealtimeEditStateBar
	resourceLabel="2세트 픽밴"
	{connectionState}
	{saveState}
	{editors}
	{lastSavedAt}
	{conflictMessage}
	onReconnect={reconnect}
	onAcceptServer={acceptServerState}
	onKeepLocal={retryLocalOperation}
/>
```

상태 타입은 `$lib/realtime/types.ts`에 있다. 실제 WebSocket 계층을 만들 때도 상태 이름 `connected | reconnecting | offline`과 `saved | saving | dirty | conflict`는 유지한다.

## Internal Components

다음 파일은 상위 보드의 레이아웃과 동작에 결합되어 있으므로 route에서 직접 import하지 않는다.

```txt
EntryCandidatePool
EntryTeamColumn
EntrySynergyRail
EntryHeadToHeadRail
ScrimPickBanResultInputTeamPanel
ScrimPickBanResultInputChampionCell
ScrimPickBanResultInputChampionHints
```

FAB의 `MiniGameModal`, `TimerModal`은 `QuickActionFab`을 통해 연다. 전적 검색은 헤더와 전적 route만 사용한다.

## Backend 연결 전 체크

1. 컴포넌트가 문서화된 업무 route에 배치되어 있는지 확인한다.
2. `$lib/fixtures` 타입을 그대로 API 계약으로 사용하지 않는다.
3. 내부 목업 state를 API snapshot 또는 parent-controlled props로 교체한다.
4. mutation은 성공, 진행 중, 실패, 권한 없음 상태를 노출한다.
5. 동시 편집 대상은 `RealtimeEditStateBar`, `expectedRevision`, 409 복구를 함께 연결한다.
6. 완료 후 이 문서의 현재 위치, 입력 계약, 목업 여부를 갱신한다.
