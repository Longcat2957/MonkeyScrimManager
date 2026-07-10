<script lang="ts">
	import Panel from '$lib/components/layout/Panel.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionBiddingWorkbench from '$lib/components/workflow/AuctionBiddingWorkbench.svelte';
	import AuctionBracketSetupBoard from '$lib/components/workflow/AuctionBracketSetupBoard.svelte';
	import AuctionMatchLineupBoard from '$lib/components/workflow/AuctionMatchLineupBoard.svelte';
	import AuctionTournamentSetupBoard from '$lib/components/workflow/AuctionTournamentSetupBoard.svelte';
	import {
		mockAuctionBracketTeams,
		mockAuctionCaptainIds,
		mockAuctionQueue,
		mockAuctionRecruitmentMembers,
		mockAuctionTeams
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="a-118 경매 컴포넌트 QA"
		description="완료된 경매 컴포넌트를 위에서부터 나열해 한 페이지에서 확인합니다."
		metrics={[
			{ label: '참가자', value: mockAuctionRecruitmentMembers.length, tone: 'info' },
			{ label: '팀', value: mockAuctionTeams.length, tone: 'info' },
			{ label: '팀장', value: mockAuctionCaptainIds.length, tone: 'warning' }
		]}
	/>

	<Panel kicker="준비" title="경매 시작 전 설정" compact>
		<AuctionTournamentSetupBoard
			initialMembers={mockAuctionRecruitmentMembers}
			initialCaptainIds={mockAuctionCaptainIds}
			teamCount={mockAuctionTeams.length}
			initialPoints={1000}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel kicker="입찰" title="경매 드래프트" compact>
		<AuctionBiddingWorkbench
			initialTeams={mockAuctionTeams}
			initialQueue={mockAuctionQueue}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel kicker="브래킷" title="경매 대진 설정" compact>
		<AuctionBracketSetupBoard
			teams={mockAuctionBracketTeams}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel kicker="라인업" title="경매 매치 라인업" compact>
		{#if mockAuctionBracketTeams[0] && mockAuctionBracketTeams[1]}
			<AuctionMatchLineupBoard
				leftTeam={mockAuctionBracketTeams[0]}
				rightTeam={mockAuctionBracketTeams[1]}
				profileIconVersion={data.championCatalog.version}
			/>
		{/if}
	</Panel>
</div>
