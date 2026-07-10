<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Metric {
		label: string;
		value: string | number;
		tone?: 'neutral' | 'info' | 'success' | 'warning' | 'danger';
	}

	interface Props {
		kicker: string;
		title: string;
		description?: string;
		metrics?: Metric[];
		actions?: Snippet;
	}

	let { kicker, title, description, metrics = [], actions }: Props = $props();

	const metricToneClass = {
		neutral: 'border-[var(--color-app-border-muted)] bg-[var(--color-app-control)] text-slate-50',
		info: 'border-cyan-400/30 bg-cyan-400/10 text-cyan-50',
		success: 'border-emerald-400/30 bg-emerald-400/10 text-emerald-50',
		warning: 'border-amber-300/30 bg-amber-300/10 text-amber-50',
		danger: 'border-red-400/30 bg-red-500/10 text-red-50'
	};
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

		{#if metrics.length > 0 || actions}
			<div class="flex flex-col gap-2 sm:flex-row sm:flex-wrap xl:ml-auto xl:justify-end">
				{#if metrics.length > 0}
					<div class="grid grid-cols-2 gap-2 sm:flex">
						{#each metrics as metric (metric.label)}
							<div class={`min-w-24 border px-3 py-2 ${metricToneClass[metric.tone ?? 'neutral']}`}>
								<p class="text-[11px] text-slate-500">{metric.label}</p>
								<p class="text-lg font-semibold">{metric.value}</p>
							</div>
						{/each}
					</div>
				{/if}
				{#if actions}
					<div class="flex items-center gap-2">{@render actions()}</div>
				{/if}
			</div>
		{/if}
	</div>
</header>
