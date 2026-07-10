<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		kicker?: string;
		title?: string;
		description?: string;
		children: Snippet;
		actions?: Snippet;
		compact?: boolean;
	}

	let { kicker, title, description, children, actions, compact = false }: Props = $props();
</script>

<section class="border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
	{#if kicker || title || description || actions}
		<div
			class="flex flex-col gap-3 border-b border-[var(--color-app-border-muted)] px-4 py-3 md:flex-row md:items-center md:justify-between"
		>
			<div class="min-w-0">
				{#if kicker}
					<p class="text-xs font-semibold uppercase text-slate-500">{kicker}</p>
				{/if}
				{#if title}
					<h2 class="mt-0.5 truncate text-sm font-semibold text-slate-100">{title}</h2>
				{/if}
				{#if description}
					<p class="mt-1 max-w-3xl text-xs leading-5 text-slate-500">{description}</p>
				{/if}
			</div>
			{#if actions}
				<div class="flex shrink-0 items-center gap-2">{@render actions()}</div>
			{/if}
		</div>
	{/if}

	<div class={compact ? '' : 'p-4'}>{@render children()}</div>
</section>
