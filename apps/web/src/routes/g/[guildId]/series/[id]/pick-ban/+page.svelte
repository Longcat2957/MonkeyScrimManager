<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import ScrimPickBanResultInputBoard from '$lib/components/workflow/ScrimPickBanResultInputBoard.svelte';
	import ScrimPickBanResultSetSummaryCard from '$lib/components/workflow/ScrimPickBanResultSetSummaryCard.svelte';
	import {
		mockScrimPickBanResultInputGames,
		mockScrimPickBanResultInputPatch,
		mockScrimPickBanResultInputTeams,
		scrimPickBanResultInputChampions
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import type { PageProps } from './$types';

	type PickBanView = 'input' | 'summary';

	let { data }: PageProps = $props();
	let activeView = $state<PickBanView>('input');

	const completedSet = mockScrimPickBanResultInputGames[0]!;
	const views: Array<{ value: PickBanView; label: string }> = [
		{ value: 'input', label: '결과 입력' },
		{ value: 'summary', label: '세트 요약' }
	];
</script>

<svelte:head>
	<title>픽밴 작업대 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="일반 내전"
		title="픽밴 작업대"
		description={`시리즈 #${data.seriesId} · BO3 · 1세트`}
	>
		{#snippet actions()}
			<div
				class="flex border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
				role="tablist"
				aria-label="픽밴 화면"
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

	{#if activeView === 'input'}
		<RealtimeEditStateBar
			resourceLabel={`시리즈 #${data.seriesId} · 1세트`}
			connectionState="connected"
			saveState="saved"
			lastSavedAt="14:52"
			editors={[
				{ id: 'operator-me', name: '탐솔', isSelf: true },
				{ id: 'operator-longcat', name: 'Longcat' }
			]}
		/>
		<ScrimPickBanResultInputBoard />
	{:else}
		<ScrimPickBanResultSetSummaryCard
			label={completedSet.label}
			blueTeam={mockScrimPickBanResultInputTeams.BLUE}
			redTeam={mockScrimPickBanResultInputTeams.RED}
			winner={completedSet.winner}
			bans={completedSet.bans}
			picks={completedSet.picks}
			champions={scrimPickBanResultInputChampions}
			version={mockScrimPickBanResultInputPatch}
		/>
	{/if}
</div>
