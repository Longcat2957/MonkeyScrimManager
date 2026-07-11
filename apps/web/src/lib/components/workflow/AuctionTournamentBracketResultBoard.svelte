<script lang="ts">
	import { Dialog, Tabs } from 'bits-ui';
	import { ChevronDown, ClipboardList, Trophy, X } from 'lucide-svelte';
	import {
		LANES,
		laneLabel,
		type Lane,
		type MockAuctionMember,
		type MockAuctionTeam
	} from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';
	import ScrimPickBanResultInputBoard from './ScrimPickBanResultInputBoard.svelte';

	type AuctionBracketMatch = {
		id: string;
		label: string;
		roundLabel: string;
		leftTeam: MockAuctionTeam | null;
		rightTeam: MockAuctionTeam | null;
		leftScore: number;
		rightScore: number;
		winnerTeamId: string | null;
		lineupsLocked: boolean;
		resultEntered: boolean;
		bestOf: 1 | 3 | 5;
	};

	type AuctionLineupSlot = {
		lane: Lane;
		member: MockAuctionMember;
	};

	interface Props {
		teams: MockAuctionTeam[];
		profileIconVersion?: string | null;
	}

	let { teams, profileIconVersion = null }: Props = $props();

	let resultModalOpen = $state(false);
	let activeMatchId = $state<string | null>(null);
	let selectedSetTab = $state('1');
	let expanded = $state(false);
	const isDirectFinal = $derived(teams.length === 2);

	const semiA = $derived<AuctionBracketMatch>({
		id: 'semi-a',
		label: '준결승 A',
		roundLabel: 'SEMIFINAL',
		leftTeam: teams[0] ?? null,
		rightTeam: teams[1] ?? null,
		leftScore: 2,
		rightScore: 1,
		winnerTeamId: teams[0]?.id ?? null,
		lineupsLocked: true,
		resultEntered: true,
		bestOf: 1
	});

	const semiB = $derived<AuctionBracketMatch>({
		id: 'semi-b',
		label: '준결승 B',
		roundLabel: 'SEMIFINAL',
		leftTeam: teams[2] ?? null,
		rightTeam: teams[3] ?? null,
		leftScore: 0,
		rightScore: 2,
		winnerTeamId: teams[3]?.id ?? null,
		lineupsLocked: true,
		resultEntered: true,
		bestOf: 1
	});

	const finalMatch = $derived<AuctionBracketMatch>({
		id: 'final',
		label: '결승',
		roundLabel: 'FINAL',
		leftTeam: isDirectFinal
			? (teams[0] ?? null)
			: semiA.winnerTeamId === semiA.leftTeam?.id
				? semiA.leftTeam
				: semiA.rightTeam,
		rightTeam: isDirectFinal
			? (teams[1] ?? null)
			: semiB.winnerTeamId === semiB.leftTeam?.id
				? semiB.leftTeam
				: semiB.rightTeam,
		leftScore: 1,
		rightScore: 0,
		winnerTeamId: isDirectFinal ? null : semiA.winnerTeamId,
		lineupsLocked: true,
		resultEntered: false,
		bestOf: 3
	});

	const semifinalMatches = $derived(isDirectFinal ? [] : [semiA, semiB]);
	const allMatches = $derived([...semifinalMatches, finalMatch]);
	const activeMatch = $derived(allMatches.find((match) => match.id === activeMatchId) ?? null);

	function openResultInput(matchId: string) {
		activeMatchId = matchId;
		selectedSetTab = '1';
		resultModalOpen = true;
	}

	function teamScore(match: AuctionBracketMatch, side: 'LEFT' | 'RIGHT'): number {
		return side === 'LEFT' ? match.leftScore : match.rightScore;
	}

	function teamForSide(match: AuctionBracketMatch, side: 'LEFT' | 'RIGHT'): MockAuctionTeam | null {
		return side === 'LEFT' ? match.leftTeam : match.rightTeam;
	}

	function winnerTeamRowClass(): string {
		return 'bg-[var(--color-app-surface)]';
	}

	function teamRowClass(match: AuctionBracketMatch, team: MockAuctionTeam | null): string {
		const base = 'bg-[var(--color-app-surface)]';
		if (!match.resultEntered || !match.winnerTeamId || !team) return base;
		return match.winnerTeamId === team.id ? winnerTeamRowClass() : `${base} opacity-50`;
	}

	function setNumbers(match: AuctionBracketMatch): number[] {
		return Array.from({ length: match.bestOf }, (_, index) => index + 1);
	}

	function assignedLineup(team: MockAuctionTeam): AuctionLineupSlot[] {
		return LANES.map((lane, index) => {
			const member = team.members[index];
			return member ? { lane, member } : null;
		}).filter((slot): slot is AuctionLineupSlot => Boolean(slot));
	}

	function matchFormatLabel(match: AuctionBracketMatch): string {
		return `Bo${match.bestOf}`;
	}

	function activeModalTitle(): string {
		if (!activeMatch) return '매치 결과 입력';
		return `${activeMatch.label} 결과 입력`;
	}

	function toggleExpanded() {
		expanded = !expanded;
	}
</script>

<section class="grid border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
	<header
		class="grid gap-3 border-b border-[var(--color-app-border-muted)] px-4 py-3 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-center"
	>
		<div class="min-w-0">
			<p class="text-xs font-semibold text-slate-500">경매 브래킷</p>
			<h2 class="mt-0.5 truncate text-sm font-semibold text-slate-100">
				토너먼트 대진 및 결과 입력
			</h2>
		</div>
		<div class="flex items-center gap-2">
			<div
				class="inline-flex h-8 items-center gap-2 border border-amber-300/35 px-2 text-xs font-semibold text-amber-100"
			>
				<Trophy size={14} />
				{finalMatch.resultEntered ? '결과 확정' : '결승 진행 중'}
			</div>
			<button
				type="button"
				class="inline-flex h-8 items-center gap-1.5 border border-[var(--color-app-border-muted)] px-2 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
				aria-expanded={expanded}
				onclick={toggleExpanded}
			>
				<ChevronDown size={14} class={expanded ? 'rotate-180 transition' : 'transition'} />
				{expanded ? '접기' : '확장'}
			</button>
		</div>
	</header>

	{#if expanded}
		<div class="grid gap-4 p-4">
			<div class="mx-auto grid w-full max-w-3xl">
				<article class="grid border border-amber-300/25 bg-[var(--color-app-surface)]">
					<header
						class="flex items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
					>
						<div class="min-w-0">
							<p class="text-[11px] font-semibold text-amber-200/70">
								{finalMatch.roundLabel} · {matchFormatLabel(finalMatch)}
							</p>
							<p class="truncate text-sm font-semibold text-slate-100">{finalMatch.label}</p>
						</div>
						<button
							type="button"
							class="inline-flex h-8 shrink-0 items-center gap-1.5 border border-cyan-400/35 px-2 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
							disabled={!finalMatch.lineupsLocked}
							onclick={() => openResultInput(finalMatch.id)}
						>
							<ClipboardList size={14} />
							결과 입력
						</button>
					</header>

					<div
						class="grid gap-px border-t border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)] sm:grid-cols-2"
					>
						{#each ['LEFT', 'RIGHT'] as side (side)}
							{@const typedSide = side as 'LEFT' | 'RIGHT'}
							{@const team = teamForSide(finalMatch, typedSide)}
							<section class={`grid content-start gap-2 p-3 ${teamRowClass(finalMatch, team)}`}>
								<div class="grid grid-cols-[minmax(0,1fr)_3rem] items-start gap-3">
									<div class="min-w-0">
										<p class="truncate text-base font-semibold text-slate-100">
											{team?.name ?? '승자 대기'}
										</p>
										{#if team}
											<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
												팀장 - {team.captain}
											</p>
										{/if}
									</div>
									<p
										class="text-right font-mono text-4xl font-semibold tabular-nums text-slate-100"
									>
										{team ? teamScore(finalMatch, typedSide) : '-'}
									</p>
								</div>
								{#if team}
									{#each assignedLineup(team) as slot (slot.member.id)}
										<div class="grid grid-cols-[2.75rem_minmax(0,1fr)] items-center gap-2">
											<div class="grid justify-items-center gap-0.5">
												<PositionIcon role={slot.lane} size="sm" />
												<span class="text-[10px] font-bold text-slate-500">
													{laneLabel[slot.lane]}
												</span>
											</div>
											<div class="min-w-0">
												<p class="truncate text-sm font-semibold text-slate-100">
													{slot.member.name}
												</p>
												<p class="truncate text-xs text-slate-500">
													{slot.member.riotTier ?? '-'} · MMR {slot.member.mmr}
												</p>
											</div>
										</div>
									{/each}
								{:else}
									<p class="text-sm font-semibold text-slate-600">라인업 대기</p>
								{/if}
							</section>
						{/each}
					</div>
				</article>
			</div>

			<div class="grid gap-4 lg:grid-cols-2">
				{#each semifinalMatches as match (match.id)}
					<article class="grid border border-cyan-400/25 bg-[var(--color-app-surface)]">
						<header
							class="flex items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
						>
							<div class="min-w-0">
								<p class="text-[11px] font-semibold text-slate-500">
									{match.roundLabel} · {matchFormatLabel(match)}
								</p>
								<p class="truncate text-sm font-semibold text-slate-100">{match.label}</p>
							</div>
							<button
								type="button"
								class="inline-flex h-8 shrink-0 items-center gap-1.5 border border-cyan-400/35 px-2 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
								disabled={!match.lineupsLocked}
								onclick={() => openResultInput(match.id)}
							>
								<ClipboardList size={14} />
								결과 입력
							</button>
						</header>

						<div
							class="grid gap-px border-t border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)] sm:grid-cols-2"
						>
							{#each ['LEFT', 'RIGHT'] as side (side)}
								{@const typedSide = side as 'LEFT' | 'RIGHT'}
								{@const team = teamForSide(match, typedSide)}
								<section class={`grid content-start gap-2 p-3 ${teamRowClass(match, team)}`}>
									<div class="grid grid-cols-[minmax(0,1fr)_3rem] items-start gap-3">
										<div class="min-w-0">
											<p class="truncate text-base font-semibold text-slate-100">
												{team?.name ?? '팀 대기'}
											</p>
											{#if team}
												<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
													팀장 - {team.captain}
												</p>
											{/if}
										</div>
										<p
											class="text-right font-mono text-3xl font-semibold tabular-nums text-slate-100"
										>
											{team ? teamScore(match, typedSide) : '-'}
										</p>
									</div>
									{#if team}
										{#each assignedLineup(team) as slot (slot.member.id)}
											<div class="grid grid-cols-[2.75rem_minmax(0,1fr)] items-center gap-2">
												<div class="grid justify-items-center gap-0.5">
													<PositionIcon role={slot.lane} size="sm" />
													<span class="text-[10px] font-bold text-slate-500">
														{laneLabel[slot.lane]}
													</span>
												</div>
												<div class="min-w-0">
													<p class="truncate text-sm font-semibold text-slate-100">
														{slot.member.name}
													</p>
													<p class="truncate text-xs text-slate-500">
														{slot.member.riotTier ?? '-'} · MMR {slot.member.mmr}
													</p>
												</div>
											</div>
										{/each}
									{:else}
										<p class="text-sm font-semibold text-slate-600">라인업 대기</p>
									{/if}
								</section>
							{/each}
						</div>
					</article>
				{/each}
			</div>
		</div>
	{:else}
		<div
			class={`grid gap-4 overflow-x-auto p-4 xl:items-center ${
				isDirectFinal
					? 'mx-auto w-full max-w-3xl'
					: 'xl:grid-cols-[minmax(22rem,1fr)_minmax(22rem,1fr)]'
			}`}
		>
			{#if !isDirectFinal}
				<div class="grid min-w-[22rem] gap-5">
					{#each semifinalMatches as match (match.id)}
						<article class="grid border border-cyan-400/25 bg-[var(--color-app-surface)]">
							<header
								class="flex items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
							>
								<div class="min-w-0">
									<p class="text-[11px] font-semibold text-slate-500">
										{match.roundLabel} · {matchFormatLabel(match)}
									</p>
									<p class="truncate text-sm font-semibold text-slate-100">{match.label}</p>
								</div>
								<button
									type="button"
									class="inline-flex h-8 shrink-0 items-center gap-1.5 border border-cyan-400/35 px-2 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
									disabled={!match.lineupsLocked}
									onclick={() => openResultInput(match.id)}
								>
									<ClipboardList size={14} />
									결과 입력
								</button>
							</header>

							<div class="grid gap-px bg-[var(--color-app-border-muted)]">
								{#each ['LEFT', 'RIGHT'] as side (side)}
									{@const typedSide = side as 'LEFT' | 'RIGHT'}
									{@const team = teamForSide(match, typedSide)}
									<div
										class={`grid grid-cols-[minmax(0,1fr)_3rem] items-center gap-3 px-3 py-2 ${teamRowClass(match, team)}`}
									>
										{#if team}
											<div class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
												<div class="min-w-0">
													<p class="truncate text-base font-semibold text-slate-100">{team.name}</p>
													<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
														팀장 - {team.captain}
													</p>
												</div>
												<div class="flex -space-x-2">
													{#each assignedLineup(team) as slot (slot.member.id)}
														<PlayerAvatar
															name={slot.member.name}
															role={slot.lane}
															avatarUrl={slot.member.avatarUrl}
															profileIconId={slot.member.profileIconId}
															{profileIconVersion}
															size="sm"
															shape="circle"
														/>
													{/each}
												</div>
											</div>
											<p
												class="text-right font-mono text-xl font-semibold tabular-nums text-slate-100"
											>
												{teamScore(match, typedSide)}
											</p>
										{:else}
											<p class="text-sm font-semibold text-slate-600">팀 대기</p>
											<p class="text-right font-mono text-xl font-semibold text-slate-600">-</p>
										{/if}
									</div>
								{/each}
							</div>
						</article>
					{/each}
				</div>
			{/if}

			<div class="grid min-w-[22rem]">
				<article class="grid border border-amber-300/25 bg-[var(--color-app-surface)]">
					<header
						class="flex items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
					>
						<div class="min-w-0">
							<p class="text-[11px] font-semibold text-amber-200/70">
								{finalMatch.roundLabel} · {matchFormatLabel(finalMatch)}
							</p>
							<p class="truncate text-sm font-semibold text-slate-100">{finalMatch.label}</p>
						</div>
						<button
							type="button"
							class="inline-flex h-8 shrink-0 items-center gap-1.5 border border-cyan-400/35 px-2 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
							disabled={!finalMatch.lineupsLocked}
							onclick={() => openResultInput(finalMatch.id)}
						>
							<ClipboardList size={14} />
							결과 입력
						</button>
					</header>

					<div class="grid gap-px bg-[var(--color-app-border-muted)]">
						{#each ['LEFT', 'RIGHT'] as side (side)}
							{@const typedSide = side as 'LEFT' | 'RIGHT'}
							{@const team = teamForSide(finalMatch, typedSide)}
							<div
								class={`grid grid-cols-[minmax(0,1fr)_3rem] items-center gap-3 px-3 py-3 ${teamRowClass(finalMatch, team)}`}
							>
								{#if team}
									<div class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
										<div class="min-w-0">
											<p class="truncate text-xl font-semibold text-slate-100">{team.name}</p>
											<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
												팀장 - {team.captain}
											</p>
										</div>
										<div class="flex -space-x-2">
											{#each assignedLineup(team) as slot (slot.member.id)}
												<PlayerAvatar
													name={slot.member.name}
													role={slot.lane}
													avatarUrl={slot.member.avatarUrl}
													profileIconId={slot.member.profileIconId}
													{profileIconVersion}
													size="md"
													shape="circle"
												/>
											{/each}
										</div>
									</div>
									<p
										class="text-right font-mono text-2xl font-semibold tabular-nums text-slate-100"
									>
										{teamScore(finalMatch, typedSide)}
									</p>
								{:else}
									<p class="text-sm font-semibold text-slate-600">승자 대기</p>
									<p class="text-right font-mono text-2xl font-semibold text-slate-600">-</p>
								{/if}
							</div>
						{/each}
					</div>
				</article>
			</div>
		</div>
	{/if}
</section>

<Dialog.Root bind:open={resultModalOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 max-h-[min(52rem,calc(100vh-2rem))] w-[calc(100vw-1.5rem)] max-w-6xl -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex h-12 items-center justify-between px-4">
				<Dialog.Title class="text-sm font-semibold text-slate-50">
					{activeModalTitle()}
				</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center rounded-sm text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
					aria-label="결과 입력 닫기"
				>
					<X size={16} />
				</Dialog.Close>
			</div>

			{#if activeMatch && activeMatch.bestOf > 1}
				<Tabs.Root bind:value={selectedSetTab}>
					<Tabs.List
						aria-label="결과 입력 세트"
						class="grid gap-1 bg-[var(--color-app-bg-soft)] p-1"
						style={`grid-template-columns: repeat(${activeMatch.bestOf}, minmax(0, 1fr));`}
					>
						{#each setNumbers(activeMatch) as setNumber (setNumber)}
							<Tabs.Trigger
								value={String(setNumber)}
								class="h-9 rounded-sm text-xs font-semibold text-slate-400 outline-none hover:text-slate-100 data-[state=active]:rounded-md data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100"
							>
								{setNumber}세트
							</Tabs.Trigger>
						{/each}
					</Tabs.List>
					{#each setNumbers(activeMatch) as setNumber (setNumber)}
						<Tabs.Content value={String(setNumber)}>
							<div class="max-h-[calc(min(52rem,100vh-2rem)-5.75rem)] overflow-y-auto">
								<ScrimPickBanResultInputBoard />
							</div>
						</Tabs.Content>
					{/each}
				</Tabs.Root>
			{:else}
				<div class="max-h-[calc(min(52rem,100vh-2rem)-3rem)] overflow-y-auto">
					<ScrimPickBanResultInputBoard />
				</div>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
