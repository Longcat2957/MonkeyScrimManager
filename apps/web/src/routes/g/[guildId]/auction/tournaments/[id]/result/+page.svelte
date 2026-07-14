<script lang="ts">
	import { resolve } from '$app/paths';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentOverviewBoard from '$lib/components/workflow/AuctionTournamentOverviewBoard.svelte';
	import type {
		AuctionTournamentResultMatch,
		AuctionTournamentResultSnapshot
	} from '$lib/contracts/auctionTournamentResult';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const tournament = $derived(data.snapshot.tournament);
	const formatLabel = $derived(tournament.format === 'TEN_PERSON' ? '10인 경매' : '20인 경매');
	const rulesLabel = $derived(formatAuctionRules(tournament.format, tournament.rules));
	const statusLabel = $derived(tournamentStatusLabel(tournament.status));
	const resultEditMatch = $derived(
		data.snapshot.permissions.canRevertResults ? latestRecordedMatch(data.snapshot.matches) : null
	);

	function tournamentStatusLabel(status: typeof tournament.status): string {
		if (status === 'BIDDING') return '입찰 진행 중';
		if (status === 'BRACKET_SETUP') return '대진 설정 중';
		if (status === 'IN_GAME') return '경기 진행 중';
		if (status === 'COMPLETED') return '완료';
		return '취소됨';
	}

	function latestRecordedMatch(
		matches: AuctionTournamentResultSnapshot['matches']
	): AuctionTournamentResultMatch | null {
		const matchOrder: Record<AuctionTournamentResultMatch['matchKey'], number> = {
			SEMIFINAL_A: 0,
			SEMIFINAL_B: 1,
			FINAL: 2
		};
		return (
			matches
				.filter((match) => match.status !== 'CANCELLED' && match.games.length > 0)
				.slice()
				.sort((left, right) => matchOrder[right.matchKey] - matchOrder[left.matchKey])[0] ?? null
		);
	}
</script>

<svelte:head>
	<title>경매 결과 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="토너먼트 결과"
		description={`${tournament.title} · 토너먼트 #${tournament.displayNumber} · ${formatLabel} · ${rulesLabel} · ${statusLabel}`}
	>
		{#snippet actions()}
			{#if resultEditMatch}
				<a
					href={resolve('/g/[guildId]/auction/matches/[matchId]', {
						guildId: data.guildId,
						matchId: resultEditMatch.id
					})}
					class="inline-flex h-9 shrink-0 items-center justify-center border border-amber-300/30 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/10"
				>
					경기 결과 수정
				</a>
			{/if}
			<a
				href={resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
					guildId: data.guildId,
					id: tournament.id
				})}
				class="inline-flex h-9 shrink-0 items-center justify-center border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
			>
				브래킷
			</a>
		{/snippet}
	</WorkspaceHeader>

	<AuctionTournamentOverviewBoard snapshot={data.snapshot} />
</div>
