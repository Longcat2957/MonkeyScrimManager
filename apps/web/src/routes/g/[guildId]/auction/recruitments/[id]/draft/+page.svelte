<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentCaptainPointSetupBoard from '$lib/components/workflow/AuctionTournamentCaptainPointSetupBoard.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import {
		mockAuctionCaptainIds,
		mockAuctionCaptainIds10,
		mockAuctionRecruitmentMembers,
		mockAuctionRecruitmentMembers10,
		mockAuctionTeams,
		mockAuctionTeams10
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const isTenPerson = $derived(data.recruitment.format === 'TEN_PERSON');
	const recruitmentMembers = $derived(
		isTenPerson ? mockAuctionRecruitmentMembers10 : mockAuctionRecruitmentMembers
	);
	const captainIds = $derived(isTenPerson ? mockAuctionCaptainIds10 : mockAuctionCaptainIds);
	const teams = $derived(isTenPerson ? mockAuctionTeams10 : mockAuctionTeams);
</script>

<svelte:head>
	<title>경매 시작 설정 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="경매 내전"
		title="경매 시작 설정"
		description={`모집 #${data.recruitmentId} · ${recruitmentMembers.length}명 · ${teams.length}팀`}
	>
		{#snippet actions()}
			<a
				href={resolve('/g/[guildId]/auction/tournaments/[id]/draft', {
					guildId: data.guildId,
					id: data.recruitment.nextTournamentId
				})}
				class="inline-flex h-10 items-center gap-2 border border-cyan-400/35 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100"
			>
				입찰 화면 <ArrowRight size={14} aria-hidden="true" />
			</a>
		{/snippet}
	</WorkspaceHeader>

	<RealtimeEditStateBar
		resourceLabel={`경매 모집 #${data.recruitmentId}`}
		connectionState="connected"
		saveState="saved"
		editors={[{ id: 'operator-me', name: '탐솔', isSelf: true }]}
	/>

	<AuctionTournamentCaptainPointSetupBoard
		initialMembers={recruitmentMembers}
		initialCaptainIds={captainIds}
		teamCount={teams.length}
		initialPoints={1000}
		profileIconVersion={data.championCatalog.version}
	/>
</div>
