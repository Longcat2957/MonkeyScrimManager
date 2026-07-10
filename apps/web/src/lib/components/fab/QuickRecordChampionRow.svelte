<script lang="ts">
	import { Target } from 'lucide-svelte';
	import { championIconUrl } from '$lib/datadragon';
	import type { QuickRecordChampion } from '$lib/fixtures/quickRecordSearch';

	interface Props {
		champion: QuickRecordChampion;
		dataDragonVersion: string;
		tone?: 'custom' | 'riot';
		iconSize?: 'sm' | 'md';
	}

	let { champion, dataDragonVersion, tone = 'custom', iconSize = 'sm' }: Props = $props();

	const iconSizeClass = {
		sm: 'size-8',
		md: 'size-9'
	};

	const toneClass = {
		custom: 'text-cyan-100',
		riot: 'text-amber-100'
	};

	function winRate(): number {
		return Math.round((champion.wins / champion.games) * 100);
	}
</script>

<div class="grid grid-cols-[auto_1fr_auto] items-center gap-2 bg-[var(--color-app-surface)] p-2">
	<img
		src={championIconUrl(champion.imageFull, dataDragonVersion)}
		alt={champion.name}
		class={`${iconSizeClass[iconSize]} object-cover`}
		loading="lazy"
	/>
	<div class="min-w-0">
		<p class="truncate text-xs font-semibold text-slate-100">{champion.name}</p>
		<p class="truncate text-[11px] text-slate-500">
			{champion.games}게임 {champion.wins}승 {champion.losses}패
		</p>
	</div>
	{#if tone === 'riot'}
		<div class={`flex items-center gap-1 text-xs font-semibold ${toneClass[tone]}`}>
			<Target size={12} />
			{winRate()}%
		</div>
	{:else}
		<p class={`text-xs font-semibold ${toneClass[tone]}`}>{winRate()}%</p>
	{/if}
</div>
