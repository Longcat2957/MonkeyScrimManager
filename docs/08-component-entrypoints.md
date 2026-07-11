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

| 상태      | 의미                                              |
| --------- | ------------------------------------------------- |
| Routed    | 실제 운영 흐름의 route에서 직접 렌더링 중         |
| Mock data | route 배치는 완료했지만 API 대신 fixture를 사용함 |
| Internal  | 상위 컴포넌트 안에서만 사용하는 구현 조각         |

현재 상위 workflow 컴포넌트는 모두 운영 route에 배치되어 있다. API가 연결되기 전까지 fixture와 local mutation을 사용하므로 route 배치 완료와 Backend 연동 완료를 구분한다.

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

| 컴포넌트                           | 운영 route                                         | 필수 입력                                                                                                 | 데이터 상태 |
| ---------------------------------- | -------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ----------- |
| `EntryDraftBoard`                  | `/g/[guildId]/recruitments/[id]/entry` 편성 탭     | `participants`, `initialAssignments`; 선택 `synergies`, `headToHead`, `targetCount`, `profileIconVersion` | Mock data   |
| `SeriesCompletedEntryRosterBoard`  | `/g/[guildId]/recruitments/[id]/entry` 미리보기 탭 | `participants`, `assignments`, `champions`; 제목, 팀 이름, 포맷은 선택                                    | Mock data   |
| `ScrimPickBanResultInputBoard`     | `/g/[guildId]/series/[id]/pick-ban` 입력 탭        | 현재 props 없음. API 연결 전에 controlled 계약으로 변경해야 한다.                                         | Mock data   |
| `ScrimPickBanResultSetSummaryCard` | `/g/[guildId]/series/[id]/pick-ban` 요약 탭        | `label`, 양 팀, `winner`, `bans`, `picks`, `champions`, `version`                                         | Mock data   |
| `SeriesResultLedgerBoard`          | `/g/[guildId]/series/[id]/result`                  | `catalog: ChampionCatalog`; 선택 `seriesId`                                                               | Mock data   |

`/g/[guildId]/series`는 상단에 모집 마감, 엔트리 편성, 픽밴, 결과 입력 상태를 보여주고 하단에 완료 시리즈를 보여준다. 편성, 픽밴, 결과 컴포넌트를 목록 route에 직접 쌓지 않는다.

## 경매 Entrypoints

`/g/[guildId]/auction`은 상단에 경매 모집과 진행 중 토너먼트, 하단에 완료된 경매를 보여준다. 각 컴포넌트는 아래 단계 route에서만 렌더링한다.

| 컴포넌트                                  | 운영 route                                                  | 필수 입력                                                                                      |
| ----------------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `AuctionTournamentCaptainPointSetupBoard` | `/g/[guildId]/auction/recruitments/[id]/draft`              | `initialMembers`; 선택 `initialCaptainIds`, `teamCount`, `initialPoints`, `profileIconVersion` |
| `AuctionTournamentBidDraftWorkbench`      | `/g/[guildId]/auction/tournaments/[id]/draft`               | `initialTeams`, `initialQueue`; 선택 `profileIconVersion`                                      |
| `AuctionTournamentBracketMatchSetupBoard` | `/g/[guildId]/auction/tournaments/[id]/bracket`             | `teams`; 선택 `profileIconVersion`                                                             |
| `AuctionMatchStartingLineupInputBoard`    | `/g/[guildId]/auction/tournaments/[id]/bracket`             | `leftTeam`, `rightTeam`; 선택 `profileIconVersion`                                             |
| `AuctionTournamentBracketResultBoard`     | `/g/[guildId]/auction/tournaments/[id]/bracket?view=result` | `teams`; 선택 `profileIconVersion`                                                             |
| `AuctionTournamentOverviewBoard`          | `/g/[guildId]/auction/tournaments/[id]/result`              | `teams`, `matches`, `championTeamId`; 선택 `profileIconVersion`                                |

이 컴포넌트들의 버튼과 drag-and-drop은 현재 local state를 변경한다. API 연동 시 화면 전체 객체를 저장하지 않고 문서화된 작은 operation과 `expectedRevision`을 전달하도록 바꾼다.

## 사용자와 운영 Entrypoints

| 컴포넌트                        | 운영 위치                                                    | 주요 계약과 상태                                                                |
| ------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `LeaderboardBoard`              | `/g/[guildId]/leaderboard`                                   | `catalog: ChampionCatalog`; 랭킹 row는 내부 목업                                |
| `RiotAccountManager`            | `/me` Riot 계정 탭                                           | `version`; 계정 목록과 mutation은 내부 목업                                     |
| `UserChampionPreferencesEditor` | `/me` 선호 챔피언 탭                                         | `catalog`, 선택 `maxPreferences`; 선호 목록과 저장은 내부 목업                  |
| `UserPrivacySettingsPanel`      | `/me` 개인정보 탭                                            | 외부 프로필, Riot ID, 외부 전적 검색 공개 설정                                  |
| `UserDefaultGuildSettingsPanel` | `/me` 대표 길드 탭                                           | 루트 접속 시 이동할 기본 guild 선택                                             |
| `OperationsQueueBoard`          | `/g/[guildId]`                                               | `items: OperationsQueueItem[]`; 작업 링크는 같은 guild의 운영 route를 가리킨다. |
| `RealtimeEditStateBar`          | 편성, 픽밴, 경매 모집 설정, 경매 드래프트, 경매 브래킷 route | resource, 연결/저장 상태, 편집자, 충돌 처리 callback                            |
| `QuickActionFab`                | `AppShell` 전역                                              | 선택 `label`; 미니게임, 전적 검색, 타이머 modal을 소유                          |
| `AppFooter`                     | `AppShell` 전역                                              | 공통 브랜드 Footer와 `/docs` 진입 링크                                          |
| `GuildMemberManagementPanel`    | `/g/[guildId]/admin` 멤버 탭                                 | guild 멤버 검색, 등록 상태, MMR 조정, 비활성화와 복구                           |
| `GuildOperationsAdminPanel`     | `/g/[guildId]/admin` 운영 탭                                 | 고정 revert checkpoint 적용과 방치 작업 정리                                    |
| `GuildSettingsAdminPanel`       | `/g/[guildId]/admin` 길드 설정 탭                            | 운영 역할, 채널, 현재 시즌 확인, 외부 공개와 시즌 초기화                        |
| `GuildAuditLogPanel`            | `/g/[guildId]/admin` 감사 로그 탭                            | 작업, 대상, 실행자 필터와 audit 상세                                            |

## Page-local UI

아래 화면은 아직 공유 workflow 컴포넌트 진입점으로 분리하지 않았다.

- `/records`: Riot 전적 검색과 상세 경기 표가 page-local이다.
- `/g/[guildId]/records`: 내전 시리즈 목록과 확장 상세가 page-local이다.
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

FAB의 `MiniGameModal`, `QuickRecordSearchModal`, `TimerModal`도 기본적으로 `QuickActionFab`을 통해 연다. 독립 route가 명시되기 전에는 route에서 modal을 직접 조립하지 않는다.

## Backend 연결 전 체크

1. 컴포넌트가 문서화된 업무 route에 배치되어 있는지 확인한다.
2. `$lib/fixtures` 타입을 그대로 API 계약으로 사용하지 않는다.
3. 내부 목업 state를 API snapshot 또는 parent-controlled props로 교체한다.
4. mutation은 성공, 진행 중, 실패, 권한 없음 상태를 노출한다.
5. 동시 편집 대상은 `RealtimeEditStateBar`, `expectedRevision`, 409 복구를 함께 연결한다.
6. 완료 후 이 문서의 현재 위치, 입력 계약, 목업 여부를 갱신한다.
