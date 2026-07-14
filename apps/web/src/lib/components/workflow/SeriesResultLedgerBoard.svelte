<script lang="ts">
	import { Swords, Trophy } from 'lucide-svelte';
	import ScrimPickBanResultSetSummaryCard from '$lib/components/workflow/ScrimPickBanResultSetSummaryCard.svelte';
	import PositionIcon from '$lib/components/workflow/PositionIcon.svelte';
	import type { SeriesResultGame, SeriesResultSnapshot } from '$lib/contracts/seriesResult';
	import type { ChampionCatalog } from '$lib/datadragon';
	import { formatSeriesRules } from '$lib/workflow/seriesRules';

	type Side = 'BLUE' | 'RED';
	type TeamId = 'TEAM_1' | 'TEAM_2';

	interface DisplayPlayer {
		id: string;
		name: string;
		lane: 'TOP' | 'JUNGLE' | 'MID' | 'BOTTOM' | 'SUPPORT';
		mmr: number;
		riotTier: string | null;
	}

	interface DisplayTeam {
		side: Side;
		name: string;
		players: DisplayPlayer[];
	}

	interface Props {
		catalog: ChampionCatalog;
		snapshot: SeriesResultSnapshot;
	}

	let { catalog, snapshot }: Props = $props();
	let selectedGameId = $state<string | null>(null);

	const champions = $derived(
		catalog.champions.map((champion) => ({
			id: champion.id,
			name: champion.name,
			imageFull: champion.imageFull
		}))
	);
	const selectedGame = $derived(
		snapshot.games.find((game) => game.id === selectedGameId) ?? snapshot.games.at(-1) ?? null
	);
	const teamOne = $derived(frozenTeam(snapshot, 'TEAM_1'));
	const teamTwo = $derived(frozenTeam(snapshot, 'TEAM_2'));
	const selectedTeams = $derived(
		selectedGame
			? {
					BLUE: teamForSide(snapshot, selectedGame, 'BLUE'),
					RED: teamForSide(snapshot, selectedGame, 'RED')
				}
			: null
	);
	const selectedMmrChanges = $derived(
		new Map((selectedGame?.mmrChanges ?? []).map((change) => [change.userId, change]))
	);
	const setNumbers = $derived(
		Array.from({ length: snapshot.series.bestOf }, (_, index) => index + 1)
	);
	const gamesBySetNumber = $derived(new Map(snapshot.games.map((game) => [game.setNumber, game])));

	function frozenTeam(snapshot: SeriesResultSnapshot, teamId: TeamId) {
		const team = snapshot.teams.find((candidate) => candidate.team === teamId);
		if (!team) throw new Error(`Missing ${teamId} in a validated series result snapshot`);
		return team;
	}

	function teamIdForSide(game: SeriesResultGame, side: Side): TeamId {
		const teamOneSide: Side = game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
		return side === teamOneSide ? 'TEAM_1' : 'TEAM_2';
	}

	function displayTeam(snapshot: SeriesResultSnapshot, teamId: TeamId, side: Side): DisplayTeam {
		const team = frozenTeam(snapshot, teamId);
		return {
			side,
			name: team.name,
			players: team.players.map((player) => ({
				id: player.id,
				name: player.displayName,
				lane: player.lane,
				mmr: player.mmr,
				riotTier: player.riotTier
			}))
		};
	}

	function teamForSide(
		snapshot: SeriesResultSnapshot,
		game: SeriesResultGame,
		side: Side
	): DisplayTeam {
		return displayTeam(snapshot, teamIdForSide(game, side), side);
	}

	function completionLabel(endedAt: string | null): string {
		return endedAt ? `${endedAt.slice(0, 10)} 종료` : '종료';
	}

	function resultStatusLabel(terminalReason: string | null): string {
		if (terminalReason === 'EARLY_WIN') return '조기 종료 · 승리 처리';
		if (terminalReason === 'EARLY_DRAW') return '조기 종료 · 무승부';
		return '완료';
	}

	function gameLabel(game: SeriesResultGame): string {
		return `${game.setNumber}세트`;
	}

	function signed(value: number): string {
		const rendered = Number.isInteger(value)
			? String(value)
			: value
					.toFixed(2)
					.replace(/\.0+$/, '')
					.replace(/(\.\d*?)0+$/, '$1');
		return value > 0 ? `+${rendered}` : rendered;
	}

	function mmrChangeFor(userId: string) {
		return selectedMmrChanges.get(userId) ?? null;
	}

	function teamMmr(teamId: TeamId): number {
		return frozenTeam(snapshot, teamId).players.reduce(
			(sum, player) => sum + (mmrChangeFor(player.id)?.delta ?? 0),
			0
		);
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<Swords size={20} class="text-cyan-300" aria-hidden="true" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">{snapshot.series.title}</h2>
				<p class="text-xs text-slate-500">
					{formatSeriesRules(snapshot.series.bestOf, snapshot.series.hardFearless)} · #{snapshot
						.series.displayNumber} · {resultStatusLabel(snapshot.series.terminalReason)}
				</p>
			</div>
		</div>
		<time class="text-xs text-slate-500" datetime={snapshot.series.endedAt ?? undefined}
			>{completionLabel(snapshot.series.endedAt)}</time
		>
	</header>

	<div class="grid border-b border-[var(--color-app-border)] lg:grid-cols-[minmax(0,1fr)_460px]">
		<div
			class="flex min-h-24 items-center justify-center gap-5 px-4 py-3 lg:border-r lg:border-[var(--color-app-border)]"
		>
			<div class="text-right">
				<strong class="block text-sm text-cyan-100">{teamOne.name}</strong>
				{#if snapshot.series.winningTeam === 'TEAM_1'}
					<span class="text-[10px] text-slate-600">승리</span>
				{/if}
			</div>
			<strong class="text-3xl tabular-nums text-slate-50">{snapshot.score.team1}</strong>
			<span class="text-xl text-slate-700">:</span>
			<strong class="text-3xl tabular-nums text-slate-50">{snapshot.score.team2}</strong>
			<div>
				<strong class="block text-sm text-rose-100">{teamTwo.name}</strong>
				{#if snapshot.series.winningTeam === 'TEAM_2'}
					<span class="text-[10px] text-slate-600">승리</span>
				{/if}
			</div>
		</div>

		<div
			class="grid gap-1 bg-[var(--color-app-bg-soft)] p-3"
			style={`grid-template-columns: repeat(${snapshot.series.bestOf}, minmax(0, 1fr));`}
		>
			{#each setNumbers as setNumber (setNumber)}
				{@const game = gamesBySetNumber.get(setNumber)}
				<button
					type="button"
					disabled={!game}
					aria-label={game ? `${setNumber}세트 결과 보기` : `${setNumber}세트 미진행`}
					title={game ? undefined : '진행되지 않은 세트'}
					class={`grid min-h-16 place-items-center rounded-sm border px-2 text-center ${
						!game
							? 'cursor-not-allowed border-[var(--color-app-border-muted)] bg-slate-950/20 text-slate-700'
							: selectedGame?.id === game.id
								? 'border-cyan-400/50 bg-cyan-400/8'
								: 'border-[var(--color-app-border)] bg-[var(--color-app-control)] hover:bg-[var(--color-app-hover)]'
					}`}
					onclick={() => {
						if (game) selectedGameId = game.id;
					}}
				>
					<strong class={`text-base font-semibold ${game ? 'text-slate-200' : 'text-slate-700'}`}>
						{setNumber}세트
					</strong>
				</button>
			{/each}
		</div>
	</div>

	{#if selectedGame && selectedTeams}
		<ScrimPickBanResultSetSummaryCard
			label={gameLabel(selectedGame)}
			blueTeam={selectedTeams.BLUE}
			redTeam={selectedTeams.RED}
			winner={selectedGame.winner}
			bans={selectedGame.bans}
			picks={selectedGame.picks}
			{champions}
			version={selectedGame.patchVersion ?? catalog.version}
		/>

		<section class="border-t border-[var(--color-app-border)]">
			<header
				class="flex min-h-11 items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-4"
			>
				<div class="flex items-center gap-2">
					<Trophy size={15} class="text-amber-300" aria-hidden="true" />
					<h3 class="text-xs font-semibold text-slate-200">MMR 정산</h3>
				</div>
				<div class="flex gap-3 text-xs font-semibold">
					<span class="text-cyan-300">{teamOne.name} {signed(teamMmr('TEAM_1'))}</span>
					<span class="text-rose-300">{teamTwo.name} {signed(teamMmr('TEAM_2'))}</span>
				</div>
			</header>
			<div class="grid gap-px bg-[var(--color-app-border-muted)] lg:grid-cols-2">
				{#each ['BLUE', 'RED'] as side (side)}
					{@const team = selectedTeams[side as Side]}
					<div class="bg-[var(--color-app-surface)]">
						{#each team.players as player (player.id)}
							{@const change = mmrChangeFor(player.id)}
							<div
								class="grid min-h-10 grid-cols-[24px_minmax(0,1fr)_64px] items-center gap-2 border-b border-[var(--color-app-border-muted)] px-3"
							>
								<PositionIcon role={player.lane} size="sm" />
								<strong class="truncate text-xs text-slate-300">{player.name}</strong>
								<b
									class:negative={Boolean(change && change.delta < 0)}
									class="text-right text-xs text-cyan-300"
									>MMR {change ? signed(change.delta) : '-'}</b
								>
							</div>
						{/each}
					</div>
				{/each}
			</div>
		</section>
	{:else}
		<section class="grid min-h-48 place-items-center px-4 text-center">
			<p class="text-sm font-semibold text-slate-500">확정된 세트 결과가 없습니다.</p>
		</section>
	{/if}
</section>

<style>
	.negative {
		color: #fda4af;
	}
</style>
