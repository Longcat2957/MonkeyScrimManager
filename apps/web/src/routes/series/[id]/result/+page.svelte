<script lang="ts">
	import SeriesResultLedgerBoard from '$lib/components/workflow/SeriesResultLedgerBoard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>시리즈 결과 | MonkeyBot</title>
</svelte:head>

<SeriesResultLedgerBoard catalog={data.championCatalog} seriesId={data.seriesId} />
