<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const sections = [
		{ id: 'overview', label: '기본 구조' },
		{ id: 'getting-started', label: '시작하기' },
		{ id: 'series', label: '일반 내전' },
		{ id: 'auction', label: '경매 내전' },
		{ id: 'records', label: '전적 검색' },
		{ id: 'leaderboard', label: '랭킹' },
		{ id: 'profile', label: '내 정보' },
		{ id: 'admin', label: '길드 관리' },
		{ id: 'realtime', label: '실시간 편집' },
		{ id: 'tools', label: '빠른 도구' }
	];
</script>

<svelte:head>
	<title>사용 문서 | MonkeyBot</title>
	<meta name="description" content="MonkeyBot 길드 내전 운영 기능 사용 문서" />
</svelte:head>

<div class="mx-auto max-w-6xl space-y-5">
	<WorkspaceHeader
		kicker="Docs"
		title="MonkeyBot 사용 문서"
		description="길드 내전 운영 기능과 화면별 흐름"
	/>

	<div class="grid gap-8 lg:grid-cols-[190px_minmax(0,1fr)] lg:items-start">
		<aside
			class="border-b border-[var(--color-app-border)] pb-4 lg:sticky lg:top-20 lg:border-r lg:border-b-0 lg:pr-5 lg:pb-0"
		>
			<nav class="grid grid-cols-2 gap-1 sm:grid-cols-3 lg:grid-cols-1" aria-label="문서 목차">
				{#each sections as section (section.id)}
					<a
						href={`#${section.id}`}
						class="flex min-h-8 items-center border-l border-transparent px-2 text-xs font-semibold text-slate-500 hover:border-cyan-400/50 hover:bg-cyan-400/6 hover:text-cyan-100"
					>
						{section.label}
					</a>
				{/each}
			</nav>
		</aside>

		<article class="docs-content min-w-0 pb-8">
			<!-- user-guide.md is trusted build-time content. -->
			<!-- eslint-disable-next-line svelte/no-at-html-tags -->
			{@html data.content}
		</article>
	</div>
</div>

<style>
	:global(.docs-content > div[id]) {
		scroll-margin-top: 5rem;
	}

	:global(.docs-content h2) {
		margin: 0 0 0.75rem;
		padding-top: 0.25rem;
		font-size: 1.25rem;
		font-weight: 700;
		color: rgb(241 245 249);
	}

	:global(.docs-content h3) {
		margin: 1.75rem 0 0.625rem;
		font-size: 0.875rem;
		font-weight: 700;
		color: rgb(207 250 254);
	}

	:global(.docs-content p) {
		margin: 0.625rem 0;
		font-size: 0.875rem;
		line-height: 1.8;
		color: rgb(148 163 184);
	}

	:global(.docs-content ul),
	:global(.docs-content ol) {
		margin: 0.75rem 0 1.5rem;
		padding-left: 1.35rem;
		font-size: 0.875rem;
		line-height: 1.8;
		color: rgb(148 163 184);
	}

	:global(.docs-content ul) {
		list-style: square;
	}

	:global(.docs-content ol) {
		list-style: decimal;
	}

	:global(.docs-content strong) {
		font-weight: 700;
		color: rgb(226 232 240);
	}

	:global(.docs-content blockquote) {
		margin: 1rem 0 2rem;
		border-left: 2px solid rgb(34 211 238 / 0.45);
		background: rgb(34 211 238 / 0.05);
		padding: 0.5rem 0.875rem;
	}

	:global(.docs-content blockquote p) {
		margin: 0;
		color: rgb(165 243 252 / 0.8);
	}

	:global(.docs-content table) {
		margin: 1rem 0 1.75rem;
		width: 100%;
		border-collapse: collapse;
		font-size: 0.8125rem;
	}

	:global(.docs-content th),
	:global(.docs-content td) {
		border: 1px solid var(--color-app-border);
		padding: 0.625rem 0.75rem;
		text-align: left;
	}

	:global(.docs-content th) {
		background: var(--color-app-bg-soft);
		font-weight: 700;
		color: rgb(203 213 225);
	}

	:global(.docs-content td) {
		color: rgb(148 163 184);
	}

	:global(.docs-content > div[id]:not(:first-child)) {
		margin-top: 2.75rem;
		border-top: 1px solid var(--color-app-border-muted);
		padding-top: 2rem;
	}

	@media (max-width: 639px) {
		:global(.docs-content table) {
			display: block;
			overflow-x: auto;
		}
	}
</style>
