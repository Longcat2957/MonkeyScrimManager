<script lang="ts">
	import { championIconUrl } from '$lib/datadragon';
	import type { ScrimPickBanResultInputChampion } from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import { findScrimPickBanResultInputChampionById } from './scrimPickBanResultInputHelpers';

	interface Props {
		label: string;
		championIds: string[];
		champions: ScrimPickBanResultInputChampion[];
		version: string;
		onPick: (championId: string) => void;
	}

	let { label, championIds, champions, version, onPick }: Props = $props();

	const visibleChampions = $derived(
		championIds
			.map((championId) => findScrimPickBanResultInputChampionById(championId, champions))
			.filter((champion) => champion !== null)
	);
</script>

<div class="grid min-w-0 grid-cols-[3.25rem_minmax(0,1fr)] items-center gap-1">
	<p class="truncate text-[11px] font-semibold text-slate-500">{label}</p>
	<div
		class="flex h-9 min-w-0 items-start gap-1 overflow-x-auto overflow-y-hidden overscroll-x-contain pb-2 pr-1"
		aria-label={`${label} 챔피언 목록`}
	>
		{#each visibleChampions as champion (champion.id)}
			<button
				type="button"
				class="grid h-7 w-24 shrink-0 grid-cols-[1.5rem_minmax(0,1fr)] items-center gap-1 rounded-sm bg-slate-900/45 px-1 text-left hover:bg-[var(--color-app-hover)]"
				title={champion.name}
				onclick={() => onPick(champion.id)}
			>
				<img
					src={championIconUrl(champion.imageFull, version)}
					alt={champion.name}
					class="size-6 object-cover"
					loading="lazy"
				/>
				<span class="truncate text-[11px] font-semibold text-slate-300">{champion.name}</span>
			</button>
		{/each}
	</div>
</div>
