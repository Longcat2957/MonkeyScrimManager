<script lang="ts">
	import { Swords, Trophy } from 'lucide-svelte';
	import ScrimPickBanResultSetSummaryCard from '$lib/components/workflow/ScrimPickBanResultSetSummaryCard.svelte';
	import PositionIcon from '$lib/components/workflow/PositionIcon.svelte';
	import type { ChampionCatalog } from '$lib/datadragon';
	import {
		mockScrimPickBanResultInputTeams,
		type ScrimPickBanResultInputGame,
		type ScrimPickBanResultInputSide
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';

	interface ResultGame extends ScrimPickBanResultInputGame {
		duration: string;
	}

	interface Props {
		catalog: ChampionCatalog;
		seriesId?: string;
	}

	function pickMap(values: Array<[string, string]>): Record<string, string | null> {
		return Object.fromEntries(values);
	}

	const games: ResultGame[] = [
		{
			id: 'result-game-1',
			label: '1세트',
			winner: 'BLUE',
			duration: '31:42',
			bans: {
				BLUE: ['Renekton', 'Sejuani', 'Azir', 'Caitlyn', 'Rakan'],
				RED: ['Aatrox', 'LeeSin', 'Orianna', 'Jinx', 'Nautilus']
			},
			picks: pickMap([
				['blue-top', 'Gnar'],
				['blue-jungle', 'Viego'],
				['blue-mid', 'Ahri'],
				['blue-bottom', 'Ezreal'],
				['blue-support', 'Thresh'],
				['red-top', 'Poppy'],
				['red-jungle', 'Vi'],
				['red-mid', 'Syndra'],
				['red-bottom', 'Kaisa'],
				['red-support', 'Alistar']
			])
		},
		{
			id: 'result-game-2',
			label: '2세트',
			winner: 'RED',
			duration: '28:16',
			bans: {
				BLUE: ['Poppy', 'Vi', 'Syndra', 'Kaisa', 'Alistar'],
				RED: ['Gnar', 'Viego', 'Ahri', 'Ezreal', 'Thresh']
			},
			picks: pickMap([
				['blue-top', 'Aatrox'],
				['blue-jungle', 'LeeSin'],
				['blue-mid', 'Orianna'],
				['blue-bottom', 'Jinx'],
				['blue-support', 'Nautilus'],
				['red-top', 'Renekton'],
				['red-jungle', 'Sejuani'],
				['red-mid', 'Azir'],
				['red-bottom', 'Ashe'],
				['red-support', 'Rakan']
			])
		},
		{
			id: 'result-game-3',
			label: '3세트',
			winner: 'BLUE',
			duration: '35:08',
			bans: {
				BLUE: ['Renekton', 'Sejuani', 'Azir', 'Ashe', 'Rakan'],
				RED: ['Aatrox', 'LeeSin', 'Orianna', 'Jinx', 'Nautilus']
			},
			picks: pickMap([
				['blue-top', 'Poppy'],
				['blue-jungle', 'Graves'],
				['blue-mid', 'Syndra'],
				['blue-bottom', 'Kaisa'],
				['blue-support', 'Alistar'],
				['red-top', 'Gnar'],
				['red-jungle', 'Viego'],
				['red-mid', 'Yone'],
				['red-bottom', 'Varus'],
				['red-support', 'Blitzcrank']
			])
		}
	];
	const mmrChanges: Record<string, number> = {
		'blue-top': 17,
		'blue-jungle': 18,
		'blue-mid': 19,
		'blue-bottom': 16,
		'blue-support': 15,
		'red-top': -16,
		'red-jungle': -18,
		'red-mid': -17,
		'red-bottom': -18,
		'red-support': -16
	};

	let { catalog, seriesId }: Props = $props();
	let selectedGameIndex = $state(2);

	const champions = $derived(
		catalog.champions.map((champion) => ({
			id: champion.id,
			name: champion.name,
			imageFull: champion.imageFull
		}))
	);
	const selectedGame = $derived(games[selectedGameIndex]);
	const blueWins = $derived(games.filter((game) => game.winner === 'BLUE').length);
	const redWins = $derived(games.filter((game) => game.winner === 'RED').length);

	function sideLabel(side: ScrimPickBanResultInputSide): string {
		return side === 'BLUE'
			? mockScrimPickBanResultInputTeams.BLUE.name
			: mockScrimPickBanResultInputTeams.RED.name;
	}

	function signed(value: number): string {
		return `${value > 0 ? '+' : ''}${value}`;
	}

	function teamMmr(side: ScrimPickBanResultInputSide): number {
		return mockScrimPickBanResultInputTeams[side].players.reduce(
			(sum, player) => sum + mmrChanges[player.id],
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
				<h2 class="truncate text-sm font-semibold text-slate-50">금요 정기 내전</h2>
				<p class="text-xs text-slate-500">일반 · BO3{seriesId ? ` · #${seriesId}` : ''} · 완료</p>
			</div>
		</div>
		<span class="text-xs text-slate-500">3일 전</span>
	</header>

	<div class="grid border-b border-[var(--color-app-border)] lg:grid-cols-[minmax(0,1fr)_460px]">
		<div
			class="flex min-h-24 items-center justify-center gap-5 px-4 py-3 lg:border-r lg:border-[var(--color-app-border)]"
		>
			<div class="text-right">
				<strong class="block text-sm text-cyan-100"
					>{mockScrimPickBanResultInputTeams.BLUE.name}</strong
				>
				<span class="text-[10px] text-slate-600">승리</span>
			</div>
			<strong class="text-3xl tabular-nums text-slate-50">{blueWins}</strong>
			<span class="text-xl text-slate-700">:</span>
			<strong class="text-3xl tabular-nums text-slate-50">{redWins}</strong>
			<div>
				<strong class="block text-sm text-rose-100"
					>{mockScrimPickBanResultInputTeams.RED.name}</strong
				>
			</div>
		</div>

		<div class="grid grid-cols-3 gap-1 bg-[var(--color-app-bg-soft)] p-3">
			{#each games as game, index (game.id)}
				<button
					type="button"
					class={`grid min-h-14 content-center gap-1 rounded-sm border px-2 text-left ${
						selectedGameIndex === index
							? 'border-cyan-400/50 bg-cyan-400/8'
							: 'border-[var(--color-app-border)] bg-[var(--color-app-control)] hover:bg-[var(--color-app-hover)]'
					}`}
					onclick={() => (selectedGameIndex = index)}
				>
					<strong class="text-xs text-slate-200">{game.label}</strong>
					<span class="text-[10px] text-slate-500"
						>{sideLabel(game.winner ?? 'BLUE')} 승 · {game.duration}</span
					>
				</button>
			{/each}
		</div>
	</div>

	<ScrimPickBanResultSetSummaryCard
		label={`${selectedGame.label} · ${selectedGame.duration}`}
		blueTeam={mockScrimPickBanResultInputTeams.BLUE}
		redTeam={mockScrimPickBanResultInputTeams.RED}
		winner={selectedGame.winner}
		bans={selectedGame.bans}
		picks={selectedGame.picks}
		{champions}
		version={catalog.version}
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
				<span class="text-cyan-300">1팀 {signed(teamMmr('BLUE'))}</span>
				<span class="text-rose-300">2팀 {signed(teamMmr('RED'))}</span>
			</div>
		</header>
		<div class="grid gap-px bg-[var(--color-app-border-muted)] lg:grid-cols-2">
			{#each ['BLUE', 'RED'] as side (side)}
				<div class="bg-[var(--color-app-surface)]">
					{#each mockScrimPickBanResultInputTeams[side as ScrimPickBanResultInputSide].players as player (player.id)}
						<div
							class="grid min-h-10 grid-cols-[24px_minmax(0,1fr)_64px] items-center gap-2 border-b border-[var(--color-app-border-muted)] px-3"
						>
							<PositionIcon role={player.lane} size="sm" />
							<strong class="truncate text-xs text-slate-300">{player.name}</strong>
							<b class:negative={mmrChanges[player.id] < 0} class="text-right text-xs text-cyan-300"
								>MMR {signed(mmrChanges[player.id])}</b
							>
						</div>
					{/each}
				</div>
			{/each}
		</div>
	</section>
</section>

<style>
	.negative {
		color: #fda4af;
	}
</style>
