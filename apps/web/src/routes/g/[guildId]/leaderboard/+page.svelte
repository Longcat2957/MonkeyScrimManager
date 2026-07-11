<script lang="ts">
	import LeaderboardBoard from '$lib/components/workflow/LeaderboardBoard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>리더보드 | MonkeyBot</title>
</svelte:head>

<LeaderboardBoard catalog={data.championCatalog} />
