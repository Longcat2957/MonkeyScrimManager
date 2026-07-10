<script lang="ts">
	import '../app.css';
	import AppShell from '$lib/components/layout/AppShell.svelte';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
</script>

<svelte:head>
	<title>MonkeyScrimManager</title>
	<meta name="description" content="Multi-guild League of Legends scrim operations console" />
</svelte:head>

<AppShell>{@render children()}</AppShell>
