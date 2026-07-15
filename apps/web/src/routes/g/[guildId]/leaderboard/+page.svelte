<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import LeaderboardBoard from '$lib/components/workflow/LeaderboardBoard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	function openPlayerRecords(playerId: string) {
		const path = resolve('/g/[guildId]/records', { guildId: data.guildId });
		const query = new globalThis.URLSearchParams({ q: playerId });
		if (data.snapshot?.season?.id) query.set('seasonId', data.snapshot.season.id);
		void goto(`${path}?${query.toString()}`);
	}
</script>

<svelte:head>
	<title>리더보드 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col">
	<LeaderboardBoard
		catalog={data.championCatalog}
		snapshot={data.snapshot}
		loadError={data.loadError}
		onOpenPlayer={openPlayerRecords}
	/>
</div>
