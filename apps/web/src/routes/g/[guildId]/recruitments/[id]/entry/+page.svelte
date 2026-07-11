<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import EntryDraftBoard from '$lib/components/workflow/EntryDraftBoard.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import SeriesCompletedEntryRosterBoard from '$lib/components/workflow/SeriesCompletedEntryRosterBoard.svelte';
	import { scrimPickBanResultInputChampions } from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import {
		mockCompletedEntryAssignments,
		mockEntryAssignments,
		mockEntryHeadToHead,
		mockEntryParticipants,
		mockEntrySynergies
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	type EntryView = 'draft' | 'preview';

	let { data }: PageProps = $props();
	let activeView = $state<EntryView>('draft');

	const views: Array<{ value: EntryView; label: string }> = [
		{ value: 'draft', label: '편성' },
		{ value: 'preview', label: '확정 미리보기' }
	];
</script>

<svelte:head>
	<title>엔트리 편성 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="일반 내전"
		title="엔트리 편성"
		description={`모집 #${data.recruitmentId} · ${mockEntryParticipants.length}명`}
	>
		{#snippet actions()}
			<div
				class="flex border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
				role="tablist"
				aria-label="엔트리 화면"
			>
				{#each views as view (view.value)}
					<button
						type="button"
						role="tab"
						aria-selected={activeView === view.value}
						onclick={() => (activeView = view.value)}
						class={`h-8 px-3 text-xs font-semibold ${
							activeView === view.value
								? 'rounded-sm bg-cyan-400/12 text-cyan-100'
								: 'text-slate-500 hover:text-slate-200'
						}`}
					>
						{view.label}
					</button>
				{/each}
			</div>
		{/snippet}
	</WorkspaceHeader>

	{#if activeView === 'draft'}
		<RealtimeEditStateBar
			resourceLabel={`모집 #${data.recruitmentId} 엔트리`}
			connectionState="connected"
			saveState="saved"
			lastSavedAt="방금"
			editors={[
				{ id: 'operator-me', name: '탐솔', isSelf: true },
				{ id: 'operator-longcat', name: 'Longcat' }
			]}
		/>

		<EntryDraftBoard
			participants={mockEntryParticipants}
			initialAssignments={mockEntryAssignments}
			synergies={mockEntrySynergies}
			headToHead={mockEntryHeadToHead}
			targetCount={10}
			profileIconVersion={data.championCatalog.version}
		/>
	{:else}
		<SeriesCompletedEntryRosterBoard
			participants={mockEntryParticipants}
			assignments={mockCompletedEntryAssignments}
			champions={scrimPickBanResultInputChampions}
			title={`모집 #${data.recruitmentId} 엔트리`}
			profileIconVersion={data.championCatalog.version}
		/>
	{/if}
</div>
