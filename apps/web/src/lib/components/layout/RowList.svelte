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
			class="hidden border-b border-slate-800 px-4 py-2 text-[11px] font-semibold uppercase text-slate-500 md:grid"
			style:grid-template-columns={columns}
		>
			{#each headings as heading (heading)}
				<span>{heading}</span>
			{/each}
		</div>
	{/if}
	<div class="divide-y divide-slate-800">{@render children()}</div>
</div>
