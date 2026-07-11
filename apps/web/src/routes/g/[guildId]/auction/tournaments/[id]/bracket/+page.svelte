<script lang="ts">
	import { untrack } from 'svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionMatchStartingLineupInputBoard from '$lib/components/workflow/AuctionMatchStartingLineupInputBoard.svelte';
	import AuctionTournamentBracketMatchSetupBoard from '$lib/components/workflow/AuctionTournamentBracketMatchSetupBoard.svelte';
	import AuctionTournamentBracketResultBoard from '$lib/components/workflow/AuctionTournamentBracketResultBoard.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import { mockAuctionBracketTeams, mockAuctionBracketTeams10 } from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	type BracketView = 'setup' | 'lineup' | 'result';

	let { data }: PageProps = $props();
	let activeView = $state<BracketView>(untrack(() => data.initialView));
	const isTenPerson = $derived(data.tournamentId.startsWith('a10-'));
	const teams = $derived(isTenPerson ? mockAuctionBracketTeams10 : mockAuctionBracketTeams);

	const views: Array<{ value: BracketView; label: string }> = [
		{ value: 'setup', label: '대진 설정' },
		{ value: 'lineup', label: '라인업' },
		{ value: 'result', label: '결과 입력' }
	];
</script>

<svelte:head>
	<title>경매 브래킷 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="경매 내전"
		title="토너먼트 브래킷"
		description={`토너먼트 #${data.tournamentId} · ${teams.length * 5}명 · ${teams.length}팀`}
	>
		{#snippet actions()}
			<div
				class="flex max-w-full overflow-x-auto border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
				role="tablist"
				aria-label="브래킷 작업"
			>
				{#each views as view (view.value)}
					<button
						type="button"
						role="tab"
						aria-selected={activeView === view.value}
						onclick={() => (activeView = view.value)}
						class={`h-8 shrink-0 px-3 text-xs font-semibold ${
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

	<RealtimeEditStateBar
		resourceLabel={`경매 #${data.tournamentId} · ${views.find((view) => view.value === activeView)?.label ?? ''}`}
		connectionState="connected"
		saveState="saved"
		editors={[{ id: 'operator-me', name: '탐솔', isSelf: true }]}
	/>

	{#if activeView === 'setup'}
		<AuctionTournamentBracketMatchSetupBoard
			{teams}
			profileIconVersion={data.championCatalog.version}
		/>
	{:else if activeView === 'lineup'}
		{#if teams[0] && teams[1]}
			<AuctionMatchStartingLineupInputBoard
				leftTeam={teams[0]}
				rightTeam={teams[1]}
				profileIconVersion={data.championCatalog.version}
			/>
		{/if}
	{:else}
		<AuctionTournamentBracketResultBoard
			{teams}
			profileIconVersion={data.championCatalog.version}
		/>
	{/if}
</div>
