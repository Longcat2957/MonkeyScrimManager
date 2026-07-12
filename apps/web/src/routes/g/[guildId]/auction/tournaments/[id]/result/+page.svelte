<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentOverviewBoard from '$lib/components/workflow/AuctionTournamentOverviewBoard.svelte';
	import {
		mockAuctionBracketTeams,
		mockAuctionBracketTeams10,
		mockAuctionChampionTeamId,
		mockAuctionChampionTeamId10,
		mockAuctionResultMatches,
		mockAuctionResultMatches10
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const isTenPerson = $derived(data.tournament.format === 'TEN_PERSON');
	const teams = $derived(isTenPerson ? mockAuctionBracketTeams10 : mockAuctionBracketTeams);
	const matches = $derived(isTenPerson ? mockAuctionResultMatches10 : mockAuctionResultMatches);
	const championTeamId = $derived(
		isTenPerson ? mockAuctionChampionTeamId10 : mockAuctionChampionTeamId
	);
</script>

<svelte:head>
	<title>경매 결과 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="경매 내전"
		title="토너먼트 결과"
		description={`토너먼트 #${data.tournamentId} · ${teams.length * 5}명 · 완료`}
	/>

	<AuctionTournamentOverviewBoard
		{teams}
		{matches}
		{championTeamId}
		profileIconVersion={data.championCatalog.version}
	/>
</div>
