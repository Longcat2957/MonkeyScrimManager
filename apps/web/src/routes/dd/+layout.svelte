<script lang="ts">
	import type { Snippet } from 'svelte';
	import AppRouteShell from '$lib/components/layout/AppRouteShell.svelte';

	let { children }: { children: Snippet } = $props();
</script>

<AppRouteShell>{@render children()}</AppRouteShell>
