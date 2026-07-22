<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();
</script>

<svelte:head>
	<title>MonkeyBot</title>
	<meta name="description" content="Multi-guild League of Legends scrim operations console" />
</svelte:head>

{@render children()}
