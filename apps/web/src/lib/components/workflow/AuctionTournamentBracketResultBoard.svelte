<script lang="ts">
	import { ChevronDown, Trophy } from 'lucide-svelte';
	import type {
		AuctionTournamentResultLineup,
		AuctionTournamentResultMatch,
		AuctionTournamentResultSnapshot,
		AuctionTournamentResultTeam,
		AuctionTournamentResultTeamMember
	} from '$lib/contracts/auctionTournamentResult';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	type MatchSide = 'LEFT' | 'RIGHT';
	type AssignedLineupRow = {
		lane: AuctionTournamentResultLineup['lane'];
		member: AuctionTournamentResultTeamMember;
	};

	interface Props {
		snapshot: AuctionTournamentResultSnapshot;
		onOpenMatch: (matchId: string) => void | Promise<void>;
	}

	let { snapshot, onOpenMatch }: Props = $props();
	let expanded = $state(false);

	const matchSides: readonly MatchSide[] = ['LEFT', 'RIGHT'];
	const laneOrder = { TOP: 0, JUNGLE: 1, MID: 2, BOTTOM: 3, SUPPORT: 4 } as const;
	const isDirectFinal = $derived(snapshot.tournament.format === 'TEN_PERSON');
	const semifinalMatches = $derived(
		snapshot.matches
			.filter((match) => match.matchKey === 'SEMIFINAL_A' || match.matchKey === 'SEMIFINAL_B')
			.slice()
			.sort((left, right) => left.matchKey.localeCompare(right.matchKey))
	);
	const finalMatch = $derived(snapshot.matches.find((match) => match.matchKey === 'FINAL') ?? null);

	function teamById(teamId: string | null): AuctionTournamentResultTeam | null {
		return teamId ? (snapshot.teams.find((team) => team.id === teamId) ?? null) : null;
	}

	function teamFor(
		match: AuctionTournamentResultMatch,
		side: MatchSide
	): AuctionTournamentResultTeam | null {
		return teamById(side === 'LEFT' ? match.leftTeamId : match.rightTeamId);
	}

	function teamScore(match: AuctionTournamentResultMatch, side: MatchSide): number {
		return side === 'LEFT' ? match.leftScore : match.rightScore;
	}

	function captainName(team: AuctionTournamentResultTeam): string {
		return (
			team.members.find((member) => member.acquiredVia === 'CAPTAIN')?.displayName ?? '팀장 미지정'
		);
	}

	function assignedLineup(
		match: AuctionTournamentResultMatch,
		team: AuctionTournamentResultTeam
	): AssignedLineupRow[] {
		const membersById = new Map(team.members.map((member) => [member.teamMemberId, member]));
		return match.lineups
			.filter((lineup) => lineup.teamId === team.id)
			.flatMap((lineup) => {
				const member = membersById.get(lineup.teamMemberId);
				return member ? [{ lane: lineup.lane, member }] : [];
			})
			.sort((left, right) => laneOrder[left.lane] - laneOrder[right.lane]);
	}

	function matchLabel(match: AuctionTournamentResultMatch): string {
		if (match.matchKey === 'SEMIFINAL_A') return '준결승 A';
		if (match.matchKey === 'SEMIFINAL_B') return '준결승 B';
		return '결승';
	}

	function matchStatusLabel(match: AuctionTournamentResultMatch): string {
		if (match.status === 'LINEUP_PENDING') return '라인업 대기';
		if (match.status === 'READY') return '시작 준비';
		if (match.status === 'IN_PROGRESS') return '진행 중';
		if (match.status === 'COMPLETED') return '완료';
		return '취소됨';
	}

	function tournamentStateLabel(): string {
		const champion = teamById(snapshot.tournament.championTeamId);
		if (champion) return `${champion.teamName} 우승`;
		if (snapshot.tournament.status === 'CANCELLED') return '토너먼트 취소됨';
		if (!finalMatch) return '대진 정보 없음';
		return matchStatusLabel(finalMatch);
	}

	function teamRowClass(
		match: AuctionTournamentResultMatch,
		team: AuctionTournamentResultTeam | null
	): string {
		if (!team) return 'bg-slate-950/35 text-slate-600';
		if (match.winnerTeamId === team.id) {
			return 'bg-emerald-400/10 text-emerald-50 ring-1 ring-inset ring-emerald-300/25';
		}
		if (match.winnerTeamId) return 'bg-slate-950/35 text-slate-400 opacity-55';
		return 'bg-[var(--color-app-surface)] text-slate-100';
	}

	function canOpenMatch(match: AuctionTournamentResultMatch): boolean {
		return match.leftTeamId !== null && match.rightTeamId !== null && match.status !== 'CANCELLED';
	}

	function openMatch(match: AuctionTournamentResultMatch): void {
		if (!canOpenMatch(match)) return;
		void onOpenMatch(match.id);
	}

	function toggleExpanded(): void {
		expanded = !expanded;
	}
</script>

{#snippet MatchRows(match: AuctionTournamentResultMatch, emphasis: boolean)}
	{#each matchSides as side (side)}
		{@const team = teamFor(match, side)}
		{@const lineup = team ? assignedLineup(match, team) : []}
		<span
			class={`grid grid-cols-[minmax(0,1fr)_3rem] items-center gap-3 px-3 ${emphasis ? 'py-3' : 'py-2'} ${teamRowClass(match, team)}`}
		>
			<span class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
				<span class="min-w-0">
					<span
						class={`block truncate font-semibold ${emphasis ? 'text-xl' : 'text-base'}`}
					>
						{team?.teamName ?? (emphasis ? '승자 대기' : '팀 대기')}
					</span>
					{#if team}
						<span class="mt-0.5 block truncate text-xs text-slate-500">
							{captainName(team)} 팀장
						</span>
					{/if}
				</span>
				{#if lineup.length > 0}
					<span class="flex shrink-0 -space-x-2" aria-label={`${team?.teamName} 확정 라인업`}>
						{#each lineup as slot (slot.member.teamMemberId)}
							<PlayerAvatar
								name={slot.member.displayName}
								role={slot.lane}
								size={emphasis ? 'md' : 'sm'}
								shape="circle"
							/>
						{/each}
					</span>
				{/if}
			</span>
			<span
				class={`text-right font-mono font-semibold tabular-nums ${emphasis ? 'text-2xl' : 'text-xl'}`}
			>
				{team ? teamScore(match, side) : '-'}
			</span>
		</span>
	{/each}
{/snippet}

{#snippet MatchCard(match: AuctionTournamentResultMatch, emphasis: boolean)}
	{@const playable = canOpenMatch(match)}
	<article
		class={`group relative grid min-w-0 overflow-hidden border bg-[var(--color-app-surface)] transition ${
			emphasis ? 'border-amber-300/25' : 'border-cyan-400/25'
		} ${
			playable ? 'cursor-pointer hover:border-cyan-300/45' : ''
		}`}
	>
		<header
			class="flex min-w-0 items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
		>
			<div class="min-w-0">
				<p class={`text-[11px] font-semibold ${emphasis ? 'text-amber-200/70' : 'text-slate-500'}`}>
					{matchLabel(match)} · BO{match.bestOf}
				</p>
				<p class="truncate text-sm font-semibold text-slate-100">{matchStatusLabel(match)}</p>
			</div>
			{#if match.winnerTeamId}
				<span class="shrink-0 text-xs font-semibold text-emerald-200">결과 확정</span>
			{/if}
		</header>

		<div class="grid gap-px bg-[var(--color-app-border-muted)]">
			{@render MatchRows(match, emphasis)}
		</div>

		{#if playable}
			<button
				type="button"
				class="absolute inset-0 z-10 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-cyan-200"
				aria-label={`${matchLabel(match)} 경기 열기`}
				onclick={() => openMatch(match)}
			></button>
		{/if}
	</article>
{/snippet}

{#snippet DetailedMatchCard(match: AuctionTournamentResultMatch, emphasis: boolean)}
	{@const playable = canOpenMatch(match)}
	<article
		class={`group relative grid min-w-0 overflow-hidden border bg-[var(--color-app-surface)] transition ${
			emphasis ? 'border-amber-300/25' : 'border-cyan-400/25'
		} ${
			playable ? 'cursor-pointer hover:border-cyan-300/45' : ''
		}`}
	>
		<header
			class="flex min-w-0 items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
		>
			<div class="min-w-0">
				<p class={`text-[11px] font-semibold ${emphasis ? 'text-amber-200/70' : 'text-slate-500'}`}>
					{matchLabel(match)} · BO{match.bestOf}
				</p>
				<p class="truncate text-sm font-semibold text-slate-100">{matchStatusLabel(match)}</p>
			</div>
			{#if match.winnerTeamId}
				<span class="shrink-0 text-xs font-semibold text-emerald-200">결과 확정</span>
			{/if}
		</header>

		<div class="grid gap-px bg-[var(--color-app-border-muted)] md:grid-cols-2">
			{#each matchSides as side (side)}
				{@const team = teamFor(match, side)}
				{@const lineup = team ? assignedLineup(match, team) : []}
				<section class={`grid min-w-0 content-start gap-3 p-3 ${teamRowClass(match, team)}`}>
					<div class="grid min-w-0 grid-cols-[minmax(0,1fr)_3rem] items-start gap-3">
						<div class="min-w-0">
							<p class="truncate text-lg font-semibold text-slate-100">
								{team?.teamName ?? (emphasis ? '승자 대기' : '팀 대기')}
							</p>
							{#if team}
								<p class="mt-0.5 truncate text-xs text-slate-500">{captainName(team)} 팀장</p>
							{/if}
						</div>
						<p class="text-right font-mono text-3xl font-semibold tabular-nums text-slate-100">
							{team ? teamScore(match, side) : '-'}
						</p>
					</div>

					{#if lineup.length > 0}
						<div class="grid gap-1.5">
							{#each lineup as slot (slot.member.teamMemberId)}
								<div
									class="grid min-w-0 grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-2 bg-slate-950/25 px-2 py-1.5"
								>
									<PositionIcon role={slot.lane} size="sm" />
									<span class="truncate text-sm font-semibold text-slate-200">
										{slot.member.displayName}
									</span>
								</div>
							{/each}
						</div>
					{:else}
						<p class="text-sm font-semibold text-slate-600">라인업 대기</p>
					{/if}
				</section>
			{/each}
		</div>

		{#if playable}
			<button
				type="button"
				class="absolute inset-0 z-10 focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-cyan-200"
				aria-label={`${matchLabel(match)} 경기 열기`}
				onclick={() => openMatch(match)}
			></button>
		{/if}
	</article>
{/snippet}

<section class="grid border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
	<header
		class="grid gap-3 border-b border-[var(--color-app-border-muted)] px-4 py-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-center"
	>
		<div class="min-w-0">
			<p class="text-xs font-semibold text-slate-500">경매 브래킷</p>
			<h2 class="mt-0.5 truncate text-sm font-semibold text-slate-100">토너먼트 현황</h2>
		</div>
		<div class="flex min-w-0 flex-wrap items-center gap-2 lg:justify-self-end">
			<div
				class="inline-flex h-8 min-w-0 max-w-full items-center gap-2 border border-amber-300/35 px-2 text-xs font-semibold text-amber-100"
			>
				<Trophy size={14} aria-hidden="true" />
				<span class="truncate">{tournamentStateLabel()}</span>
			</div>
			<button
				type="button"
				class="inline-flex h-8 shrink-0 items-center gap-1.5 border border-[var(--color-app-border-muted)] px-2 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
				aria-controls="auction-tournament-bracket-details"
				aria-expanded={expanded}
				onclick={toggleExpanded}
			>
				<ChevronDown
					size={14}
					class={expanded ? 'rotate-180 transition-transform' : 'transition-transform'}
					aria-hidden="true"
				/>
				{expanded ? '간략히 보기' : '상세보기'}
			</button>
		</div>
	</header>

	{#if finalMatch}
		{#if expanded}
			<div id="auction-tournament-bracket-details" class="grid min-w-0 gap-4 p-4">
				<div class="mx-auto grid w-full max-w-5xl min-w-0">
					{@render DetailedMatchCard(finalMatch, true)}
				</div>

				{#if !isDirectFinal}
					<div class="grid min-w-0 gap-4 xl:grid-cols-2">
						{#each semifinalMatches as match (match.id)}
							{@render DetailedMatchCard(match, false)}
						{/each}
					</div>
				{/if}
			</div>
		{:else}
			<div
				class={`grid min-w-0 gap-4 overflow-x-auto p-4 2xl:items-center ${
					isDirectFinal
						? 'mx-auto w-full max-w-3xl'
						: '2xl:grid-cols-[minmax(24rem,1fr)_minmax(28rem,1fr)]'
				}`}
			>
				{#if !isDirectFinal}
					<div class="grid min-w-[22rem] gap-5 2xl:min-w-[24rem]">
						{#each semifinalMatches as match (match.id)}
							{@render MatchCard(match, false)}
						{/each}
					</div>
				{/if}

				<div class="grid min-w-[22rem] 2xl:min-w-[28rem]">
					{@render MatchCard(finalMatch, true)}
				</div>
			</div>
		{/if}
	{:else}
		<div class="grid min-h-48 place-items-center px-4 text-center text-sm font-semibold text-slate-500">
			표시할 대진 정보가 없습니다.
		</div>
	{/if}
</section>
