<script lang="ts">
	import { Trophy } from 'lucide-svelte';
	import { championIconUrl } from '$lib/datadragon';
	import PositionIcon from './PositionIcon.svelte';

	type ScrimPickBanResultSetSummarySide = 'BLUE' | 'RED';
	type ScrimPickBanResultSetSummaryLane = 'TOP' | 'JUNGLE' | 'MID' | 'BOTTOM' | 'SUPPORT';

	interface ScrimPickBanResultSetSummaryChampion {
		id: string;
		name: string;
		imageFull: string;
	}

	interface ScrimPickBanResultSetSummaryPlayer {
		id: string;
		name: string;
		lane: ScrimPickBanResultSetSummaryLane;
		mmr?: number;
		riotTier?: string | null;
	}

	interface ScrimPickBanResultSetSummaryTeam {
		side: ScrimPickBanResultSetSummarySide;
		name: string;
		players: ScrimPickBanResultSetSummaryPlayer[];
	}

	interface Props {
		label: string;
		blueTeam: ScrimPickBanResultSetSummaryTeam;
		redTeam: ScrimPickBanResultSetSummaryTeam;
		winner: ScrimPickBanResultSetSummarySide | null;
		bans: Record<ScrimPickBanResultSetSummarySide, Array<string | null>>;
		picks: Record<string, string | null>;
		champions: ScrimPickBanResultSetSummaryChampion[];
		version: string;
	}

	let { label, blueTeam, redTeam, winner, bans, picks, champions, version }: Props = $props();

	const teams = $derived([blueTeam, redTeam]);
	function championById(championId: string | null): ScrimPickBanResultSetSummaryChampion | null {
		if (!championId) return null;
		return champions.find((champion) => champion.id === championId) ?? null;
	}

	function sideLabel(side: ScrimPickBanResultSetSummarySide): string {
		return side === 'BLUE' ? 'BLUE' : 'RED';
	}

	function sideToneClass(side: ScrimPickBanResultSetSummarySide): string {
		return side === 'BLUE' ? 'border-blue-300/35 text-blue-100' : 'border-red-300/35 text-red-100';
	}

	function teamPanelClass(side: ScrimPickBanResultSetSummarySide): string {
		const base = 'bg-[var(--color-app-surface)]';
		if (!winner) return base;
		if (winner !== side) return `${base} opacity-60`;
		return base;
	}

	function playerMeta(player: ScrimPickBanResultSetSummaryPlayer): string {
		return `MMR ${player.mmr ?? '-'} · ${player.riotTier ?? '-'}`;
	}
</script>

<article class="grid overflow-hidden bg-[var(--color-app-surface)]">
	<header
		class="grid gap-2 border-b border-[var(--color-app-border-muted)] px-3 py-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center"
	>
		<div class="min-w-0">
			<p class="text-xs font-semibold text-slate-500">세트 결과</p>
			<p class="mt-0.5 truncate text-sm font-semibold text-slate-100">{label}</p>
		</div>
		<div class="flex flex-wrap items-center gap-1.5 sm:justify-end">
			{#if winner}
				<span
					class="inline-flex h-7 items-center gap-1.5 border border-emerald-400/35 bg-emerald-400/10 px-2 text-[11px] font-semibold text-emerald-100"
				>
					<Trophy size={13} />
					{sideLabel(winner)} 승리
				</span>
			{/if}
		</div>
	</header>

	<div class="grid gap-px bg-[var(--color-app-border-muted)] lg:grid-cols-2">
		{#each teams as team (team.side)}
			<section class={`grid min-w-0 gap-3 p-3 ${teamPanelClass(team.side)}`}>
				<div class="flex min-h-9 items-center justify-between gap-2">
					<div class="flex min-w-0 items-center gap-2">
						<span class={`border px-2 py-1 text-[11px] font-semibold ${sideToneClass(team.side)}`}>
							{sideLabel(team.side)}
						</span>
						<p class="truncate text-sm font-semibold text-slate-100">{team.name}</p>
					</div>
				</div>

				<div class="grid min-w-0 grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-2">
					<div class="text-[11px] font-semibold text-slate-500">BAN</div>
					<div class="flex min-w-0 flex-wrap gap-1.5">
						{#each bans[team.side] as championId, index (`${team.side}-ban-${index}`)}
							{@const champion = championById(championId)}
							<div
								class="grid size-9 shrink-0 place-items-center bg-[var(--color-app-surface)]"
								title={champion?.name ?? '미입력'}
							>
								{#if champion}
									<img
										src={championIconUrl(champion.imageFull, version)}
										alt={champion.name}
										class="size-full object-cover"
										loading="lazy"
									/>
								{:else}
									<span class="size-full bg-slate-900/80"></span>
								{/if}
							</div>
						{/each}
					</div>
				</div>

				<div class="grid gap-1.5">
					{#each team.players as player (player.id)}
						{@const champion = championById(picks[player.id] ?? null)}
						<div
							class="grid min-h-16 min-w-0 grid-cols-[2.75rem_minmax(0,1fr)] items-center gap-2 border border-[var(--color-app-border-muted)] px-2 py-1"
						>
							<span class="grid h-full place-items-center text-slate-300">
								<PositionIcon role={player.lane} size="md" />
							</span>
							<div class="grid min-w-0 grid-cols-[3.25rem_minmax(0,1fr)] items-center gap-3 p-1.5">
								{#if champion}
									<img
										src={championIconUrl(champion.imageFull, version)}
										alt={champion.name}
										title={champion.name}
										class="size-12 object-cover"
										loading="lazy"
									/>
									<div class="min-w-0">
										<p class="truncate text-base font-semibold text-slate-100">{player.name}</p>
										<p class="mt-0.5 truncate text-sm font-semibold text-slate-500">
											{playerMeta(player)}
										</p>
									</div>
								{:else}
									<span class="size-12 bg-slate-900/80"></span>
									<div class="min-w-0">
										<p class="truncate text-base font-semibold text-slate-100">{player.name}</p>
										<p class="mt-0.5 truncate text-sm font-semibold text-slate-500">
											{playerMeta(player)}
										</p>
									</div>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</section>
		{/each}
	</div>
</article>
