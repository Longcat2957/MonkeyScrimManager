<script lang="ts">
	import {
		type ScrimPickBanResultInputChampion,
		type ScrimPickBanResultInputSide,
		type ScrimPickBanResultInputTeam
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import PositionIcon from './PositionIcon.svelte';
	import {
		scrimPickBanResultInputBanKey,
		scrimPickBanResultInputCellId,
		type ScrimPickBanResultInputActiveCell
	} from './scrimPickBanResultInputHelpers';
	import ScrimPickBanResultInputChampionCell from './ScrimPickBanResultInputChampionCell.svelte';
	import ScrimPickBanResultInputChampionHints from './ScrimPickBanResultInputChampionHints.svelte';

	interface Props {
		team: ScrimPickBanResultInputTeam;
		bans: Array<string | null>;
		picks: Record<string, string | null>;
		activeCell: ScrimPickBanResultInputActiveCell;
		champions: ScrimPickBanResultInputChampion[];
		version: string;
		onActiveCell: (cell: ScrimPickBanResultInputActiveCell) => void;
		onBan: (side: ScrimPickBanResultInputSide, index: number, championId: string | null) => void;
		onPick: (playerId: string, championId: string | null) => void;
		onNavigate?: (cell: ScrimPickBanResultInputActiveCell, direction: -1 | 1) => void;
	}

	let {
		team,
		bans,
		picks,
		activeCell,
		champions,
		version,
		onActiveCell,
		onBan,
		onPick,
		onNavigate
	}: Props = $props();

	function isActive(kind: ScrimPickBanResultInputActiveCell['kind'], key: string): boolean {
		return activeCell.kind === kind && activeCell.key === key;
	}

	function sideClass(side: ScrimPickBanResultInputSide): string {
		return side === 'BLUE' ? 'border-blue-300/30 text-blue-100' : 'border-red-300/30 text-red-100';
	}

	function pickRowClass(playerId: string, picked: string | null): string {
		if (isActive('PICK', playerId))
			return 'z-20 rounded-md bg-cyan-300/45 shadow-[0_0_0_1px_rgba(103,232,249,0.2)]';
		if (picked) return 'rounded-md bg-[var(--color-app-border-muted)]';
		return 'rounded-sm bg-[var(--color-app-border-muted)]';
	}
</script>

<section class="min-w-0 bg-[var(--color-app-surface)]">
	<div
		class="flex h-11 items-center justify-between overflow-hidden px-2 py-2 sm:h-[3.125rem] sm:px-3"
	>
		<p class="text-sm font-semibold text-slate-100">{team.name}</p>
		<span class={`border px-2 py-1 text-[11px] font-semibold ${sideClass(team.side)}`}>
			{team.side}
		</span>
	</div>

	<div class="grid gap-1.5 px-1.5 py-1 sm:px-2">
		<div
			class="relative z-10 mb-1 grid h-12 grid-cols-[2.5rem_repeat(5,minmax(7rem,1fr))] gap-px rounded-sm bg-[var(--color-app-border-muted)] p-px"
		>
			<div class="grid place-items-center bg-slate-950/35 text-xs font-semibold text-slate-500">
				밴
			</div>
			{#each bans as ban, index (`${team.side}-ban-${index}`)}
				{@const key = scrimPickBanResultInputBanKey(team.side, index)}
				<ScrimPickBanResultInputChampionCell
					value={ban}
					{champions}
					{version}
					compact
					embedded
					active={isActive('BAN', key)}
					inputId={scrimPickBanResultInputCellId({ kind: 'BAN', key })}
					inputLabel={`${team.name} ${team.side} 밴 ${index + 1}`}
					onFocus={() => onActiveCell({ kind: 'BAN', key })}
					onCommit={(championId) => onBan(team.side, index, championId)}
					onNavigate={onNavigate
						? (direction) => onNavigate({ kind: 'BAN', key }, direction)
						: undefined}
				/>
			{/each}
		</div>

		<div class="grid grid-rows-[repeat(5,4.25rem)] gap-1.5 sm:grid-rows-[repeat(5,4.75rem)]">
			{#each team.players as player (player.id)}
				{@const picked = picks[player.id] ?? null}
				<div
					class={`relative grid h-[4.25rem] min-h-0 grid-cols-[2rem_7rem_minmax(10rem,0.9fr)_minmax(18rem,1.2fr)] items-stretch gap-px p-px text-left transition sm:h-[4.75rem] sm:grid-cols-[48px_8rem_minmax(11rem,0.9fr)_minmax(22rem,1.2fr)] ${pickRowClass(player.id, picked)}`}
				>
					<span class="grid place-items-center bg-slate-950/35 text-slate-300">
						<PositionIcon role={player.lane} size="md" />
					</span>

					<button
						type="button"
						class="grid min-w-0 content-center bg-slate-950/35 px-2 text-left hover:bg-slate-900/70"
						onclick={() => onActiveCell({ kind: 'PICK', key: player.id })}
					>
						<span class="truncate text-sm font-semibold text-slate-100">{player.name}</span>
					</button>

					<ScrimPickBanResultInputChampionCell
						value={picked}
						{champions}
						{version}
						embedded
						active={isActive('PICK', player.id)}
						inputId={scrimPickBanResultInputCellId({ kind: 'PICK', key: player.id })}
						inputLabel={`${team.name} ${player.name} 픽`}
						onFocus={() => onActiveCell({ kind: 'PICK', key: player.id })}
						onCommit={(championId) => onPick(player.id, championId)}
						onNavigate={onNavigate
							? (direction) => onNavigate({ kind: 'PICK', key: player.id }, direction)
							: undefined}
					/>

					<div class="grid min-w-0 content-center gap-1 bg-slate-950/20 px-1.5">
						<ScrimPickBanResultInputChampionHints
							label="선호"
							championIds={player.preferredChampions}
							{champions}
							{version}
							onPick={(championId) => {
								onActiveCell({ kind: 'PICK', key: player.id });
								onPick(player.id, championId);
							}}
						/>
						<ScrimPickBanResultInputChampionHints
							label="주 사용"
							championIds={player.frequentChampions}
							{champions}
							{version}
							onPick={(championId) => {
								onActiveCell({ kind: 'PICK', key: player.id });
								onPick(player.id, championId);
							}}
						/>
					</div>
				</div>
			{/each}
		</div>
	</div>
</section>
