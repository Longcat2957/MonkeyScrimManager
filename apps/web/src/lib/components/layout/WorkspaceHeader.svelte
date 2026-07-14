<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		kicker: string;
		title: string;
		description?: string;
		actions?: Snippet;
	}

	let { kicker, title, description, actions }: Props = $props();
</script>

<header class="border-b border-[var(--color-app-border-muted)] pb-4">
	<div class="flex flex-col gap-4 xl:flex-row xl:items-end">
		<div class="min-w-0">
			<p class="text-xs font-semibold uppercase text-cyan-200">{kicker}</p>
			<h1 class="mt-1 max-w-5xl text-2xl font-semibold tracking-normal text-slate-50">{title}</h1>
			{#if description}
				<p class="mt-2 max-w-4xl text-sm leading-6 text-slate-400">{description}</p>
			{/if}
		</div>

		{#if actions}
			<div class="flex flex-col gap-2 sm:flex-row sm:flex-wrap xl:ml-auto xl:justify-end">
				<div class="flex w-full items-center justify-end gap-2 xl:w-auto">{@render actions()}</div>
			</div>
		{/if}
	</div>
</header>
