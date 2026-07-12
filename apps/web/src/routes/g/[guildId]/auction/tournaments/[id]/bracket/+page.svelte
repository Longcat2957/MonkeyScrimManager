<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { untrack } from 'svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionMatchStartingLineupInputBoard from '$lib/components/workflow/AuctionMatchStartingLineupInputBoard.svelte';
	import AuctionTournamentBracketMatchSetupBoard from '$lib/components/workflow/AuctionTournamentBracketMatchSetupBoard.svelte';
	import AuctionTournamentBracketResultBoard from '$lib/components/workflow/AuctionTournamentBracketResultBoard.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import {
		createMockBracketMatches,
		createMockBracketSetupSnapshot
	} from '$lib/fixtures/auctionWorkflow';
	import { mockAuctionBracketTeams, mockAuctionBracketTeams10 } from '$lib/fixtures/workflow';
	import type {
		AuctionBracketSetupCommand,
		AuctionMatchResultCommand
	} from '$lib/realtime/auction';
	import { scoreAuctionGames } from '$lib/realtime/auction';
	import type { PageProps } from './$types';

	type BracketView = 'setup' | 'lineup' | 'result';

	let { data }: PageProps = $props();
	const activeView = $derived<BracketView>(data.initialView);
	const teams = untrack(() =>
		data.tournament.format === 'TEN_PERSON' ? mockAuctionBracketTeams10 : mockAuctionBracketTeams
	);
	let setupSnapshot = $state(createMockBracketSetupSnapshot(teams));
	let matches = $state(createMockBracketMatches(teams));

	const views: Array<{ value: BracketView; label: string }> = [
		{ value: 'setup', label: '대진 설정' },
		{ value: 'lineup', label: '라인업' },
		{ value: 'result', label: '결과 입력' }
	];

	function handleSetupCommand(command: AuctionBracketSetupCommand) {
		if (command.expectedRevision !== setupSnapshot.revision) return;

		if (command.kind === 'LOCK_BRACKET') {
			setupSnapshot = {
				...setupSnapshot,
				locked: true,
				revision: setupSnapshot.revision + 1
			};
			return;
		}

		const assignments = { ...setupSnapshot.assignments };
		if (command.kind === 'ASSIGN_TEAM') {
			for (const slot of Object.keys(assignments) as Array<keyof typeof assignments>) {
				if (assignments[slot] === command.teamId) assignments[slot] = null;
			}
			assignments[command.slot] = command.teamId;
		} else {
			assignments[command.slot] = null;
		}

		setupSnapshot = {
			revision: setupSnapshot.revision + 1,
			locked: setupSnapshot.locked,
			assignments
		};
	}

	function handleResultCommand(command: AuctionMatchResultCommand) {
		matches = matches.map((match) => {
			if (match.id !== command.matchId || match.revision !== command.expectedRevision) return match;

			const games = [...match.games];
			games[command.setNumber - 1] = command.game;
			const { leftScore, rightScore } = scoreAuctionGames(games);
			const winsRequired = Math.floor(match.bestOf / 2) + 1;
			const resultEntered = leftScore >= winsRequired || rightScore >= winsRequired;
			const winnerTeamId = resultEntered
				? leftScore > rightScore
					? match.leftTeamId
					: match.rightTeamId
				: null;

			return {
				...match,
				games,
				leftScore,
				rightScore,
				winnerTeamId,
				resultEntered,
				revision: match.revision + 1
			};
		});
	}

	function selectView(view: BracketView) {
		const url = new globalThis.URL(page.url);
		url.searchParams.set('view', view);
		void goto(`${url.pathname}${url.search}`, { replaceState: true, noScroll: true });
	}
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
						onclick={() => selectView(view.value)}
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
			snapshot={setupSnapshot}
			onCommand={handleSetupCommand}
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
			{matches}
			onSubmitResult={handleResultCommand}
			profileIconVersion={data.championCatalog.version}
		/>
	{/if}
</div>
