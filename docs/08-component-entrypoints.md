# MonkeyBot Component Entrypoints

작성일: 2026-07-11

## 목적

이 문서는 이미 만든 UI를 route에서 다시 조립할 때 사용하는 컴포넌트 진입점의 단일 기준이다.

- 어떤 컴포넌트를 직접 import해야 하는지 확인한다.
- 현재 확인 가능한 route와 최종 업무 route를 구분한다.
- 필요한 props와 데이터 출처를 확인한다.
- 상위 컴포넌트 대신 내부 조각을 잘못 호출하는 일을 막는다.

컴포넌트의 화면 책임은 이 문서, route의 업무 책임은 [Frontend Routing](routing/frontend.md), 동시 편집 계약은 [Realtime Collaboration](07-realtime-collaboration.md)을 따른다.

## 상태 표기

| 상태        | 의미                                              |
| ----------- | ------------------------------------------------- |
| Route       | 이름이 명시된 route에서 직접 렌더링 중            |
| QA          | 목업 데이터로 상호작용과 레이아웃을 확인하는 상태 |
| Placeholder | 최종 route는 있으나 아직 컴포넌트가 연결되지 않음 |
| Internal    | 상위 컴포넌트 안에서만 사용하는 구현 조각         |

`Route`는 API 연동 완료를 의미하지 않는다. 현재 `LeaderboardBoard`, `RiotAccountManager`, `UserChampionPreferencesEditor` 등은 실제 route에 배치되어 있지만 데이터와 mutation은 아직 컴포넌트 내부 목업이다.

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

| 컴포넌트                           | 현재 위치                   | 최종 위치                              | 필수 입력                                                                                                 |
| ---------------------------------- | --------------------------- | -------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `RecruitmentEntryQueue`            | `/recruitments` Route       | `/recruitments`                        | `items: MockRecruitmentQueueItem[]`                                                                       |
| `EntryDraftBoard`                  | `/series` QA                | `/recruitments/[id]/entry` Placeholder | `participants`, `initialAssignments`; 선택 `synergies`, `headToHead`, `targetCount`, `profileIconVersion` |
| `SeriesCompletedEntryRosterBoard`  | `/series` QA                | 배치 route 미정                        | `participants`, `assignments`, `champions`; 제목, 팀 이름, 포맷은 선택                                    |
| `ScrimPickBanResultInputBoard`     | `/series` QA                | `/series/[id]/pick-ban` Placeholder    | 현재 props 없음. 내부 fixture를 사용하므로 API 연결 전에 controlled 계약으로 변경해야 한다.               |
| `ScrimPickBanResultSetSummaryCard` | `/series` QA                | 픽밴/결과 화면 내부 반복 항목          | `label`, 양 팀, `winner`, `bans`, `picks`, `champions`, `version`                                         |
| `SeriesResultLedgerBoard`          | `/series/[id]/result` Route | 동일                                   | `catalog: ChampionCatalog`; 선택 `seriesId`                                                               |

현재 `/series`는 진행 중 일반 내전 컴포넌트를 모아 확인하는 QA 화면이다. 최종 업무 흐름에서는 엔트리, 픽밴, 결과를 위 표의 개별 route로 분리한다.

## 경매 Entrypoints

현재 경매 컴포넌트는 모두 `/auction/[id]`에서 목업으로 함께 확인한다. 이 route는 완료된 컴포넌트의 QA 진입점이며 최종 경매 URL 구조가 아니다.

| 컴포넌트                                  | 최종 업무 route                     | 필수 입력                                                                                      |
| ----------------------------------------- | ----------------------------------- | ---------------------------------------------------------------------------------------------- |
| `AuctionTournamentCaptainPointSetupBoard` | `/auction/recruitments/[id]/draft`  | `initialMembers`; 선택 `initialCaptainIds`, `teamCount`, `initialPoints`, `profileIconVersion` |
| `AuctionTournamentBidDraftWorkbench`      | `/auction/tournaments/[id]/draft`   | `initialTeams`, `initialQueue`; 선택 `profileIconVersion`                                      |
| `AuctionTournamentBracketMatchSetupBoard` | `/auction/tournaments/[id]/bracket` | `teams`; 선택 `profileIconVersion`                                                             |
| `AuctionMatchStartingLineupInputBoard`    | `/auction/tournaments/[id]/bracket` | `leftTeam`, `rightTeam`; 선택 `profileIconVersion`                                             |
| `AuctionTournamentBracketResultBoard`     | `/auction/tournaments/[id]/bracket` | `teams`; 선택 `profileIconVersion`                                                             |
| `AuctionTournamentOverviewBoard`          | `/auction/tournaments/[id]/result`  | `teams`, `matches`, `championTeamId`; 선택 `profileIconVersion`                                |

이 컴포넌트들의 버튼과 drag-and-drop은 현재 local state를 변경한다. API 연동 시 화면 전체 객체를 저장하지 않고 문서화된 작은 operation과 `expectedRevision`을 전달하도록 바꾼다.

## 사용자와 운영 Entrypoints

| 컴포넌트                        | 현재 위치                  | 주요 계약과 상태                                               |
| ------------------------------- | -------------------------- | -------------------------------------------------------------- |
| `LeaderboardBoard`              | `/leaderboard` Route       | `catalog: ChampionCatalog`; 랭킹 row는 내부 목업               |
| `RiotAccountManager`            | `/me/riot-accounts` Route  | `version`; 계정 목록과 mutation은 내부 목업                    |
| `UserChampionPreferencesEditor` | `/users/[discordId]` Route | `catalog`, 선택 `maxPreferences`; 선호 목록과 저장은 내부 목업 |
| `OperationsQueueBoard`          | `/dev/components` QA       | `items: OperationsQueueItem[]`; 실제 배치 route는 미정         |
| `RealtimeEditStateBar`          | `/dev/components` QA       | resource, 연결/저장 상태, 편집자, 충돌 처리 callback           |
| `QuickActionFab`                | `/dev/fab` QA              | 선택 `label`; 미니게임, 전적 검색, 타이머 modal을 소유         |

`/dev/components`는 현재 개발 또는 검토 중인 컴포넌트만 둔다. 완료 후에는 사용자가 지정한 QA route나 실제 업무 route로 옮기고 이 표를 갱신한다.

## Page-local UI

아래 화면은 아직 공유 workflow 컴포넌트 진입점으로 분리하지 않았다.

- `/records`: Riot 전적 검색과 상세 경기 표가 page-local이다.
- `/g/[guildId]/records`: 내전 시리즈 목록과 확장 상세가 page-local이다.
- `/series/completed`: 완료 시리즈 목록이 page-local이다.
- `/admin/status`: 운영 상태 UI가 page-local이다.

다른 route에서 같은 업무 UI가 실제로 필요해질 때만 상위 컴포넌트로 추출한다.

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

<LeaderboardBoard catalog={data.championCatalog} />
```

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

상태 타입은 현재 `$lib/fixtures/componentDevelopment.ts`에 있다. 실제 WebSocket 계층을 만들 때 fixture 타입을 runtime 모듈로 이동하되 상태 이름 `connected | reconnecting | offline`과 `saved | saving | dirty | conflict`는 유지한다.

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

`AuctionBidTargetAssignmentBoard`는 현재 route나 상위 컴포넌트에서 호출되지 않는다. 경매 드래프트 진입점으로 사용하지 않고 `AuctionTournamentBidDraftWorkbench`를 사용한다.

FAB의 `MiniGameModal`, `QuickRecordSearchModal`, `TimerModal`도 기본적으로 `QuickActionFab`을 통해 연다. 독립 route가 명시되기 전에는 route에서 modal을 직접 조립하지 않는다.

## Backend 연결 전 체크

1. 현재 route와 최종 업무 route가 같은지 확인한다.
2. `$lib/fixtures` 타입을 그대로 API 계약으로 사용하지 않는다.
3. 내부 목업 state를 API snapshot 또는 parent-controlled props로 교체한다.
4. mutation은 성공, 진행 중, 실패, 권한 없음 상태를 노출한다.
5. 동시 편집 대상은 `RealtimeEditStateBar`, `expectedRevision`, 409 복구를 함께 연결한다.
6. 완료 후 이 문서의 현재 위치, 입력 계약, 목업 여부를 갱신한다.
