<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentBidDraftWorkbench from '$lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import {
		mockAuctionQueue,
		mockAuctionQueue10,
		mockAuctionTeams,
		mockAuctionTeams10
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const isTenPerson = $derived(data.tournamentId.startsWith('a10-'));
	const teams = $derived(isTenPerson ? mockAuctionTeams10 : mockAuctionTeams);
	const queue = $derived(isTenPerson ? mockAuctionQueue10 : mockAuctionQueue);
</script>

<svelte:head>
	<title>경매 드래프트 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="경매 내전"
		title="경매 드래프트"
		description={`토너먼트 #${data.tournamentId} · ${teams.length * 5}명 · ${teams.length}팀`}
	>
		{#snippet actions()}
			<a
				href={resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
					guildId: data.guildId,
					id: data.tournamentId
				})}
				class="inline-flex h-10 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 hover:text-slate-50"
			>
				브래킷 <ArrowRight size={14} aria-hidden="true" />
			</a>
		{/snippet}
	</WorkspaceHeader>

	<RealtimeEditStateBar
		resourceLabel={`경매 #${data.tournamentId} 입찰`}
		connectionState="connected"
		saveState="saved"
		lastSavedAt="방금"
		editors={[
			{ id: 'operator-me', name: '탐솔', isSelf: true },
			{ id: 'captain-b', name: 'Longcat' }
		]}
	/>

	<AuctionTournamentBidDraftWorkbench
		initialTeams={teams}
		initialQueue={queue}
		profileIconVersion={data.championCatalog.version}
	/>
</div>
