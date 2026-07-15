<script lang="ts">
	import { page } from '$app/state';
	import '../app.css';
	import AppShell from '$lib/components/layout/AppShell.svelte';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
	const usesPublicShell = $derived(['/landing', '/install'].includes(page.route.id ?? ''));
</script>

<svelte:head>
	<title>MonkeyBot</title>
	<meta name="description" content="Multi-guild League of Legends scrim operations console" />
</svelte:head>

{#if usesPublicShell}
	{@render children()}
{:else}
	<AppShell>{@render children()}</AppShell>
{/if}
