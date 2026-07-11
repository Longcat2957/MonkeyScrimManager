<script lang="ts">
	import RiotAccountManager from '$lib/components/workflow/RiotAccountManager.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>Riot 계정 | MonkeyBot</title>
</svelte:head>

<div class="mx-auto max-w-5xl">
	<RiotAccountManager version={data.championCatalog.version} />
</div>
