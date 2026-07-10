<script lang="ts">
	import { championIconUrl, type ChampionCatalogEntry } from '$lib/datadragon';

	interface Props {
		champion: ChampionCatalogEntry;
		version: string;
		size?: 'xs' | 'sm' | 'md';
		showName?: boolean;
	}

	let { champion, version, size = 'sm', showName = false }: Props = $props();

	const sizeClass = {
		xs: 'size-6',
		sm: 'size-8',
		md: 'size-10'
	};
</script>

<span class="inline-flex min-w-0 items-center gap-2">
	<img
		src={championIconUrl(champion, version)}
		alt={champion.name}
		class={`${sizeClass[size]} shrink-0 border border-slate-700 bg-slate-950 object-cover`}
		loading="lazy"
		decoding="async"
	/>
	{#if showName}
		<span class="min-w-0 truncate text-slate-100">{champion.name}</span>
	{/if}
</span>
