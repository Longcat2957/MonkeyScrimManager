<script lang="ts">
	import { ChevronDown, Trophy } from 'lucide-svelte';
	import type { ChampionCatalog } from '$lib/datadragon';
	import type { SeriesPickBanSnapshot } from '$lib/contracts/seriesPickBan';
	import ScrimPickBanResultSetSummaryCard from './ScrimPickBanResultSetSummaryCard.svelte';

	type Side = 'BLUE' | 'RED';
	type TeamId = 'TEAM_1' | 'TEAM_2';
	type ConfirmedGame = SeriesPickBanSnapshot['games'][number];

	interface DisplayTeam {
		side: Side;
		name: string;
		players: Array<{
			id: string;
			name: string;
			lane: 'TOP' | 'JUNGLE' | 'MID' | 'BOTTOM' | 'SUPPORT';
		}>;
	}

	interface Props {
		snapshot: SeriesPickBanSnapshot;
		catalog: ChampionCatalog;
	}

	let { snapshot, catalog }: Props = $props();
	let expanded = $state(false);

	const confirmedGames = $derived(
		[...snapshot.games].sort((left, right) => left.setNumber - right.setNumber)
	);

	function teamIdForSide(game: ConfirmedGame, side: Side): TeamId {
		const teamOneSide: Side = game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
		return side === teamOneSide ? 'TEAM_1' : 'TEAM_2';
	}

	function teamForSide(game: ConfirmedGame, side: Side): DisplayTeam {
		const teamId = teamIdForSide(game, side);
		const team = snapshot.teams.find((candidate) => candidate.team === teamId);
		if (!team) throw new Error(`Missing ${teamId} in a validated pick-ban snapshot`);

		return {
			side,
			name: team.name,
			players: team.players.map((player) => ({
				id: player.id,
				name: player.displayName,
				lane: player.lane
			}))
		};
	}
</script>

{#if confirmedGames.length > 0}
	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<button
			type="button"
			class="flex min-h-10 w-full items-center justify-between gap-3 px-3 text-left hover:bg-[var(--color-app-hover)]"
			aria-expanded={expanded}
			aria-controls="confirmed-set-results"
			onclick={() => (expanded = !expanded)}
		>
			<span class="flex min-w-0 items-center gap-2">
				<Trophy size={15} class="shrink-0 text-amber-300" aria-hidden="true" />
				<span class="truncate text-xs font-semibold text-slate-200">입력된 세트 결과</span>
				<span class="shrink-0 text-[11px] font-medium text-slate-500">
					{confirmedGames.length}세트 확정
				</span>
			</span>
			<span
				class="inline-flex shrink-0 items-center gap-1.5 text-[11px] font-semibold text-slate-400"
			>
				{expanded ? '접기' : '확인'}
				<ChevronDown
					size={15}
					class={`transition-transform ${expanded ? 'rotate-180' : ''}`}
					aria-hidden="true"
				/>
			</span>
		</button>

		{#if expanded}
			<div
				id="confirmed-set-results"
				class="divide-y divide-[var(--color-app-border-muted)] border-t border-[var(--color-app-border-muted)]"
			>
				{#each confirmedGames as game (game.setNumber)}
					{@const blueTeam = teamForSide(game, 'BLUE')}
					{@const redTeam = teamForSide(game, 'RED')}
					<ScrimPickBanResultSetSummaryCard
						label={`${game.setNumber}세트`}
						{blueTeam}
						{redTeam}
						winner={game.winner}
						bans={game.bans}
						picks={game.picks}
						champions={catalog.champions}
						version={game.patchVersion ?? catalog.version}
					/>
				{/each}
			</div>
		{/if}
	</section>
{/if}
