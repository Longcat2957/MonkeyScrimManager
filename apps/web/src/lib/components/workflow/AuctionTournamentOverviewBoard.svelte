<script lang="ts">
	import { Medal, ReceiptText, RotateCcw, Swords, Trophy } from 'lucide-svelte';
	import {
		LANES,
		laneLabel,
		type Lane,
		type MockAuctionMember,
		type MockAuctionResultMatch,
		type MockAuctionTeam
	} from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		teams: MockAuctionTeam[];
		matches: MockAuctionResultMatch[];
		championTeamId: string;
		profileIconVersion?: string | null;
	}

	let { teams, matches, championTeamId, profileIconVersion = null }: Props = $props();

	type MatchSide = 'LEFT' | 'RIGHT';

	const MATCH_SIDES = ['LEFT', 'RIGHT'] as const satisfies readonly MatchSide[];
	const FINAL_ROUND = 'FINAL';
	const CAPTAIN_ACQUISITION = 'CAPTAIN';
	const MANUAL_ACQUISITION = 'MANUAL';
	const POINT_SUFFIX = 'P';
	const VERSUS_LABEL = 'VS';
	const NO_TEAM_LABEL = '팀 없음';
	const NO_FINAL_RESULT_LABEL = '결승 결과 없음';
	const AUCTION_RESULT_NOTE = '경매 결과도 guild MMR에 반영됩니다.';

	const finalMatch = $derived(
		matches.find((match) => match.round === FINAL_ROUND) ?? matches.at(-1) ?? null
	);
	const playerLedger = $derived(
		teams
			.flatMap((team) =>
				team.members.map((member) => ({
					team,
					member,
					record: teamRecordCounts(team.id),
					mmrDelta: teamMmrDelta(team.id)
				}))
			)
			.sort((a, b) => {
				if (a.team.id !== b.team.id) return a.team.name.localeCompare(b.team.name);
				return LANES.indexOf(a.member.role) - LANES.indexOf(b.member.role);
			})
	);

	function teamById(teamId: string): MockAuctionTeam | null {
		return teams.find((team) => team.id === teamId) ?? null;
	}

	function teamSpent(team: MockAuctionTeam): number {
		return team.members.reduce((sum, member) => sum + (member.acquiredAtPoints ?? 0), 0);
	}

	function teamRecord(teamId: string): string {
		const record = teamRecordCounts(teamId);
		return `${record.wins}승 ${record.losses}패`;
	}

	function teamRecordCounts(teamId: string): { wins: number; losses: number } {
		let wins = 0;
		let losses = 0;
		for (const match of matches) {
			if (match.leftTeamId !== teamId && match.rightTeamId !== teamId) continue;
			if (match.winnerTeamId === teamId) wins += 1;
			else losses += 1;
		}
		return { wins, losses };
	}

	function teamMmrDelta(teamId: string): number {
		const record = teamRecordCounts(teamId);
		return record.wins * 18 - record.losses * 14;
	}

	function laneMember(team: MockAuctionTeam, lane: Lane): MockAuctionMember | null {
		return team.members.find((member) => member.role === lane) ?? null;
	}

	function memberPriceLabel(member: MockAuctionMember): string {
		if (member.acquiredVia === CAPTAIN_ACQUISITION) return '팀장';
		if (member.acquiredVia === MANUAL_ACQUISITION) return '수동';
		return pointLabel(member.acquiredAtPoints ?? member.price);
	}

	function pointLabel(points: number): string {
		return `${points}${POINT_SUFFIX}`;
	}

	function mmrDeltaLabel(delta: number): string {
		if (delta > 0) return `+${delta}`;
		return String(delta);
	}

	function mmrDeltaClass(delta: number): string {
		if (delta > 0) return 'text-emerald-300';
		if (delta < 0) return 'text-rose-300';
		return 'text-slate-400';
	}

	function scoreFor(match: MockAuctionResultMatch, side: MatchSide): number {
		return side === 'LEFT' ? match.leftScore : match.rightScore;
	}

	function teamFor(match: MockAuctionResultMatch, side: MatchSide): MockAuctionTeam | null {
		return teamById(side === 'LEFT' ? match.leftTeamId : match.rightTeamId);
	}

	function matchSideClass(match: MockAuctionResultMatch, team: MockAuctionTeam | null): string {
		if (!team) return 'border-transparent bg-slate-950/35 text-slate-600';
		if (match.winnerTeamId === team.id) {
			return 'border-emerald-300/35 bg-emerald-400/10 text-emerald-50';
		}
		return 'border-transparent bg-slate-950/35 text-slate-300';
	}

	function teamPanelClass(team: MockAuctionTeam): string {
		if (team.id === championTeamId) {
			return 'border-amber-300/40 bg-amber-300/[0.075]';
		}
		return 'border-[var(--color-app-border-muted)] bg-slate-950/20';
	}

	function finalMatchSummary(match: MockAuctionResultMatch | null): string {
		if (!match) return NO_FINAL_RESULT_LABEL;
		return `${match.label} ${match.leftScore}:${match.rightScore}`;
	}

	function matchFormatLabel(match: MockAuctionResultMatch): string {
		return `Bo${match.bestOf}`;
	}
</script>

<div class="grid gap-2 pb-2">
	<div class="grid gap-2 xl:grid-cols-[minmax(20rem,28rem)_minmax(0,1fr)]">
		<section
			class="grid min-h-[34rem] grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<div class="flex items-center justify-between gap-3">
					<div class="min-w-0">
						<p class="text-base font-semibold text-slate-100">대진 결과</p>
						<p class="mt-1 text-xs text-slate-500">
							{finalMatchSummary(finalMatch)}
						</p>
					</div>
					<Swords size={18} class="text-cyan-200" />
				</div>
			</div>

			<div
				class="grid min-h-0 content-start gap-2 overflow-x-hidden overflow-y-auto overscroll-contain p-3 pr-[13px]"
			>
				{#each matches as match (match.id)}
					<section class="grid gap-2 bg-slate-950/20 p-3">
						<div class="flex items-center justify-between gap-3">
							<p class="text-sm font-semibold text-cyan-100">{match.label}</p>
							<p class="text-xs font-semibold text-slate-500">
								{matchFormatLabel(match)}
							</p>
						</div>

						<div class="grid grid-cols-[minmax(0,1fr)_3.25rem_minmax(0,1fr)] items-stretch gap-2">
							{#each MATCH_SIDES as side (side)}
								{@const team = teamFor(match, side)}
								<div class={`grid min-h-20 border px-3 py-2 ${matchSideClass(match, team)}`}>
									<div class="flex min-w-0 items-center justify-between gap-3">
										<div class="min-w-0">
											<p class="truncate text-lg font-semibold">{team?.name ?? NO_TEAM_LABEL}</p>
											<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
												{team ? team.captain : '-'} 팀장
											</p>
										</div>
										<p class="font-mono text-3xl font-black tabular-nums">
											{scoreFor(match, side)}
										</p>
									</div>
								</div>

								{#if side === 'LEFT'}
									<div class="grid place-items-center text-sm font-black text-slate-500">
										{VERSUS_LABEL}
									</div>
								{/if}
							{/each}
						</div>
					</section>
				{/each}
			</div>
		</section>

		<section
			class="grid min-h-[34rem] grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<div class="flex items-center justify-between gap-3">
					<div class="min-w-0">
						<p class="text-base font-semibold text-slate-100">팀 구성</p>
						<p class="mt-1 text-xs text-slate-500">최종 로스터와 포인트 사용 내역</p>
					</div>
					<Medal size={18} class="text-amber-200" />
				</div>
			</div>

			<div
				class="grid min-h-0 content-start gap-2 overflow-x-hidden overflow-y-auto overscroll-contain p-3 pr-[13px]"
			>
				{#each teams as team (team.id)}
					<section class={`grid gap-3 border p-3 ${teamPanelClass(team)}`}>
						<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
							<div class="min-w-0">
								<div class="flex min-w-0 items-center gap-2">
									{#if team.id === championTeamId}
										<Trophy size={18} class="shrink-0 text-amber-200" />
									{/if}
									<h3 class="truncate text-xl font-semibold text-slate-50">{team.name}</h3>
								</div>
								<p class="mt-1 truncate text-xs font-semibold text-slate-500">
									{team.captain} 팀장 · {teamRecord(team.id)}
								</p>
							</div>
							<div class="grid grid-cols-2 gap-1.5 text-right">
								<div class="bg-slate-950/30 px-2 py-1.5">
									<p class="text-[10px] font-semibold text-slate-500">사용</p>
									<p class="font-mono text-sm font-bold tabular-nums text-slate-100">
										{pointLabel(teamSpent(team))}
									</p>
								</div>
								<div class="bg-slate-950/30 px-2 py-1.5">
									<p class="text-[10px] font-semibold text-slate-500">잔여</p>
									<p class="font-mono text-sm font-bold tabular-nums text-slate-100">
										{pointLabel(team.points)}
									</p>
								</div>
							</div>
						</div>

						<div class="grid gap-1.5 lg:grid-cols-5">
							{#each LANES as lane (lane)}
								{@const member = laneMember(team, lane)}
								<div class="grid min-h-24 content-start gap-2 bg-slate-950/25 p-2">
									<div class="flex items-center justify-between gap-2">
										<PositionIcon role={lane} size="sm" />
										<span class="text-[10px] font-bold text-slate-500">{laneLabel[lane]}</span>
									</div>
									{#if member}
										<div class="flex min-w-0 items-center gap-2">
											<PlayerAvatar
												name={member.name}
												role={member.role}
												avatarUrl={member.avatarUrl}
												profileIconId={member.profileIconId}
												{profileIconVersion}
												size="sm"
											/>
											<div class="min-w-0">
												<p class="truncate text-sm font-semibold text-slate-100">{member.name}</p>
												<p class="truncate text-xs font-semibold text-slate-500">
													{memberPriceLabel(member)}
												</p>
											</div>
										</div>
									{:else}
										<p class="text-sm font-semibold text-slate-600">미배치</p>
									{/if}
								</div>
							{/each}
						</div>
					</section>
				{/each}
			</div>
		</section>
	</div>

	<section
		class="grid border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
	>
		<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
			<div class="flex items-center justify-between gap-3">
				<div class="min-w-0">
					<p class="text-base font-semibold text-slate-100">참가자 정산</p>
					<p class="mt-1 text-xs text-slate-500">현재 결과 기준의 포인트와 MMR 반영 내역</p>
				</div>
				<ReceiptText size={18} class="text-slate-400" />
			</div>
		</div>

		<div class="overflow-x-auto">
			<div class="min-w-[840px]">
				<div
					class="grid grid-cols-[3rem_minmax(0,1.15fr)_minmax(0,0.75fr)_5rem_5.5rem_5.5rem_6rem] border-b border-[var(--color-app-border-muted)] px-3 py-2 text-[11px] font-bold uppercase text-slate-500"
				>
					<span>#</span>
					<span>참가자</span>
					<span>팀</span>
					<span>라인</span>
					<span class="text-right">낙찰</span>
					<span class="text-right">전적</span>
					<span class="text-right">MMR 변화</span>
				</div>

				{#each playerLedger as item, index (item.member.id)}
					<div
						class="grid min-h-14 grid-cols-[3rem_minmax(0,1.15fr)_minmax(0,0.75fr)_5rem_5.5rem_5.5rem_6rem] items-center border-b border-[var(--color-app-border-muted)] px-3 py-2 text-sm last:border-b-0"
					>
						<span class="font-mono text-xs font-bold tabular-nums text-slate-500">
							{index + 1}
						</span>
						<span class="flex min-w-0 items-center gap-2">
							<PlayerAvatar
								name={item.member.name}
								role={item.member.role}
								avatarUrl={item.member.avatarUrl}
								profileIconId={item.member.profileIconId}
								{profileIconVersion}
								size="sm"
							/>
							<span class="min-w-0">
								<span class="block truncate font-semibold text-slate-100">{item.member.name}</span>
								<span class="block truncate text-xs text-slate-500">
									{item.member.riotTier ?? '-'}
								</span>
							</span>
						</span>
						<span class="truncate font-semibold text-slate-200">{item.team.name}</span>
						<span
							class="inline-flex min-w-0 items-center gap-1 text-xs font-semibold text-slate-400"
						>
							<PositionIcon role={item.member.role} size="sm" />
							<span>{laneLabel[item.member.role]}</span>
						</span>
						<span class="text-right font-mono font-bold tabular-nums text-amber-100">
							{memberPriceLabel(item.member)}
						</span>
						<span class="text-right font-mono font-bold tabular-nums text-slate-300">
							{item.record.wins}-{item.record.losses}
						</span>
						<span
							class={`text-right font-mono font-bold tabular-nums ${mmrDeltaClass(item.mmrDelta)}`}
						>
							{mmrDeltaLabel(item.mmrDelta)}
						</span>
					</div>
				{/each}
			</div>
		</div>
	</section>

	<div
		class="flex flex-col gap-2 border-y border-[var(--color-app-border-muted)] bg-slate-950/20 p-3 sm:flex-row sm:items-center sm:justify-between"
	>
		<p class="text-xs font-semibold text-slate-500">
			{AUCTION_RESULT_NOTE}
		</p>
		<button
			type="button"
			class="inline-flex h-10 items-center justify-center gap-2 border border-amber-300/30 bg-amber-300/10 px-3 text-sm font-semibold text-amber-100 hover:bg-amber-300/15"
		>
			<RotateCcw size={15} />
			결과 입력 되돌리기
		</button>
	</div>
</div>
