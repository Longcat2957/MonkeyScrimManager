<script lang="ts">
	import Panel from '$lib/components/layout/Panel.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import EntryDraftBoard from '$lib/components/workflow/EntryDraftBoard.svelte';
	import ScrimPickBanResultInputBoard from '$lib/components/workflow/ScrimPickBanResultInputBoard.svelte';
	import ScrimPickBanResultSetSummaryCard from '$lib/components/workflow/ScrimPickBanResultSetSummaryCard.svelte';
	import SeriesCompletedEntryRosterBoard from '$lib/components/workflow/SeriesCompletedEntryRosterBoard.svelte';
	import {
		mockScrimPickBanResultInputGames,
		mockScrimPickBanResultInputPatch,
		mockScrimPickBanResultInputTeams,
		scrimPickBanResultInputChampions
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import {
		mockCompletedEntryAssignments,
		mockEntryAssignments,
		mockEntryHeadToHead,
		mockEntryParticipants,
		mockEntrySynergies
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const completedSetResult = mockScrimPickBanResultInputGames[0]!;
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="일반 내전"
		title="진행 중 시리즈 작업대"
		description="엔트리 편성 DnD와 1세트 픽밴/결과 입력 컴포넌트를 한 화면에서 확인합니다."
		metrics={[
			{ label: '참가자', value: mockEntryParticipants.length, tone: 'info' },
			{ label: '배치', value: Object.keys(mockEntryAssignments).length, tone: 'neutral' },
			{ label: '세트', value: 1, tone: 'success' }
		]}
	/>

	<Panel kicker="엔트리" title="팀/라인 편성" compact>
		<EntryDraftBoard
			participants={mockEntryParticipants}
			initialAssignments={mockEntryAssignments}
			synergies={mockEntrySynergies}
			headToHead={mockEntryHeadToHead}
			targetCount={10}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel kicker="엔트리" title="확정 엔트리 보드" compact>
		<SeriesCompletedEntryRosterBoard
			participants={mockEntryParticipants}
			assignments={mockCompletedEntryAssignments}
			champions={scrimPickBanResultInputChampions}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel kicker="픽밴" title="1세트 입력" compact>
		<ScrimPickBanResultInputBoard />
	</Panel>

	<Panel kicker="결과" title="1세트 결과 요약" compact>
		<ScrimPickBanResultSetSummaryCard
			label={completedSetResult.label}
			blueTeam={mockScrimPickBanResultInputTeams.BLUE}
			redTeam={mockScrimPickBanResultInputTeams.RED}
			winner={completedSetResult.winner}
			bans={completedSetResult.bans}
			picks={completedSetResult.picks}
			champions={scrimPickBanResultInputChampions}
			version={mockScrimPickBanResultInputPatch}
		/>
	</Panel>
</div>
