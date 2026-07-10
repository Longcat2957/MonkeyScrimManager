<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		columns?: string;
		headings?: string[];
		children: Snippet;
	}

	let { columns = '1fr', headings = [], children }: Props = $props();
</script>

<div class="overflow-hidden">
	{#if headings.length > 0}
		<div
			class="hidden border-b border-[var(--color-app-border-muted)] px-4 py-2 text-[11px] font-semibold uppercase text-slate-500 md:grid"
			style:grid-template-columns={columns}
		>
			{#each headings as heading (heading)}
				<span>{heading}</span>
			{/each}
		</div>
	{/if}
	<div class="divide-y divide-[var(--color-app-border-muted)]">{@render children()}</div>
</div>
