<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';

	interface DocumentSection {
		id: string;
		label: string;
	}

	interface Props {
		kicker: string;
		title: string;
		description: string;
		content: string;
		sections: DocumentSection[];
		navigationLabel: string;
		dateLabel?: string;
		dateValue?: string;
	}

	let {
		kicker,
		title,
		description,
		content,
		sections,
		navigationLabel,
		dateLabel,
		dateValue
	}: Props = $props();
</script>

<div class="mx-auto max-w-6xl space-y-5">
	<WorkspaceHeader {kicker} {title} {description} />

	<div class="grid gap-8 lg:grid-cols-[190px_minmax(0,1fr)] lg:items-start">
		<aside
			class="border-b border-[var(--color-app-border)] pb-4 lg:sticky lg:top-20 lg:border-r lg:border-b-0 lg:pr-5 lg:pb-0"
		>
			<nav
				class="grid grid-cols-2 gap-1 sm:grid-cols-3 lg:grid-cols-1"
				aria-label={navigationLabel}
			>
				{#each sections as section (section.id)}
					<a
						href={`#${section.id}`}
						class="flex min-h-8 items-center border-l border-transparent px-2 text-xs font-semibold text-slate-500 hover:border-cyan-400/50 hover:bg-cyan-400/6 hover:text-cyan-100"
					>
						{section.label}
					</a>
				{/each}
			</nav>

			{#if dateLabel && dateValue}
				<div class="mt-4 border-t border-[var(--color-app-border-muted)] px-2 pt-3">
					<p class="text-[10px] font-semibold text-slate-600">{dateLabel}</p>
					<p class="mt-1 text-xs font-medium text-slate-400">{dateValue}</p>
				</div>
			{/if}
		</aside>

		<article class="static-markdown-content min-w-0 pb-8">
			<!-- Only trusted repository-owned Markdown is rendered here. -->
			<!-- eslint-disable-next-line svelte/no-at-html-tags -->
			{@html content}
		</article>
	</div>
</div>

<style>
	:global(.static-markdown-content > div[id]) {
		scroll-margin-top: 5rem;
	}

	:global(.static-markdown-content h2) {
		margin: 0 0 0.75rem;
		padding-top: 0.25rem;
		font-size: 1.25rem;
		font-weight: 700;
		color: rgb(241 245 249);
	}

	:global(.static-markdown-content h3) {
		margin: 1.75rem 0 0.625rem;
		font-size: 0.875rem;
		font-weight: 700;
		color: rgb(207 250 254);
	}

	:global(.static-markdown-content h4) {
		margin: 1.25rem 0 0.5rem;
		font-size: 0.8125rem;
		font-weight: 700;
		color: rgb(203 213 225);
	}

	:global(.static-markdown-content p) {
		margin: 0.625rem 0;
		font-size: 0.875rem;
		line-height: 1.8;
		color: rgb(148 163 184);
	}

	:global(.static-markdown-content ul),
	:global(.static-markdown-content ol) {
		margin: 0.75rem 0 1.5rem;
		padding-left: 1.35rem;
		font-size: 0.875rem;
		line-height: 1.8;
		color: rgb(148 163 184);
	}

	:global(.static-markdown-content ul) {
		list-style: square;
	}

	:global(.static-markdown-content ol) {
		list-style: decimal;
	}

	:global(.static-markdown-content li + li) {
		margin-top: 0.25rem;
	}

	:global(.static-markdown-content strong) {
		font-weight: 700;
		color: rgb(226 232 240);
	}

	:global(.static-markdown-content a) {
		color: rgb(165 243 252);
		text-decoration: underline;
		text-decoration-color: rgb(34 211 238 / 0.35);
		text-underline-offset: 0.2rem;
	}

	:global(.static-markdown-content a:hover) {
		color: rgb(236 254 255);
		text-decoration-color: rgb(103 232 249 / 0.75);
	}

	:global(.static-markdown-content code) {
		border: 1px solid var(--color-app-border-muted);
		background: var(--color-app-bg-soft);
		padding: 0.1rem 0.3rem;
		font-size: 0.8em;
		color: rgb(207 250 254);
	}

	:global(.static-markdown-content blockquote) {
		margin: 1rem 0 2rem;
		border-left: 2px solid rgb(34 211 238 / 0.45);
		background: rgb(34 211 238 / 0.05);
		padding: 0.5rem 0.875rem;
	}

	:global(.static-markdown-content blockquote p) {
		margin: 0;
		color: rgb(165 243 252 / 0.8);
	}

	:global(.static-markdown-content table) {
		margin: 1rem 0 1.75rem;
		width: 100%;
		border-collapse: collapse;
		font-size: 0.8125rem;
	}

	:global(.static-markdown-content th),
	:global(.static-markdown-content td) {
		border: 1px solid var(--color-app-border);
		padding: 0.625rem 0.75rem;
		text-align: left;
		vertical-align: top;
	}

	:global(.static-markdown-content th) {
		background: var(--color-app-bg-soft);
		font-weight: 700;
		color: rgb(203 213 225);
	}

	:global(.static-markdown-content td) {
		color: rgb(148 163 184);
	}

	:global(.static-markdown-content > div[id]:not(:first-child)) {
		margin-top: 2.75rem;
		border-top: 1px solid var(--color-app-border-muted);
		padding-top: 2rem;
	}

	@media (max-width: 639px) {
		:global(.static-markdown-content table) {
			display: block;
			overflow-x: auto;
		}
	}
</style>
