<script lang="ts">
	import { resolve } from '$app/paths';
	import { Search } from 'lucide-svelte';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import RowList from '$lib/components/layout/RowList.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="Shared capability"
		title="Data Dragon champion catalog"
		description="챔피언 얼굴, 이름, id, key, 검색 slug를 모든 작업대가 같은 방식으로 쓰기 위한 기반 route입니다."
		metrics={[
			{ label: 'Version', value: data.championCatalog.version, tone: 'info' },
			{ label: 'Locale', value: data.championCatalog.locale },
			{ label: 'Results', value: data.results.length, tone: 'success' }
		]}
	>
		{#snippet actions()}
			<form
				method="GET"
				class="flex h-9 w-full items-center gap-2 border border-slate-800 bg-slate-950/70 px-3 sm:w-80"
			>
				<Search size={15} class="text-slate-500" />
				<input
					name="q"
					value={data.query}
					placeholder="name, id, key, tag"
					class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
				/>
			</form>
		{/snippet}
	</WorkspaceHeader>

	<Panel kicker="Champion rows" title="Catalog lookup" compact>
		<RowList
			columns="56px minmax(0,1fr) 84px 88px 120px"
			headings={['Icon', 'Champion', 'Key', 'idSlug', 'Tags']}
		>
			{#each data.results as champion (champion.id)}
				<a
					href={resolve('/dd/catalog') + `?q=${encodeURIComponent(champion.id)}`}
					class="grid gap-3 px-4 py-3 hover:bg-slate-900/80 md:grid-cols-[56px_minmax(0,1fr)_84px_88px_120px] md:items-center"
				>
					<ChampionFace {champion} version={data.championCatalog.version} size="sm" />
					<div class="min-w-0">
						<p class="truncate text-sm font-semibold text-slate-50">{champion.name}</p>
						<p class="text-xs text-slate-500">{champion.id}</p>
					</div>
					<p class="text-xs font-semibold text-cyan-100">{champion.key}</p>
					<p class="text-xs text-slate-400">{champion.idSlug}</p>
					<p class="truncate text-xs text-slate-500">{champion.tags.join(' / ')}</p>
				</a>
			{:else}
				<p class="px-4 py-5 text-sm text-slate-500">No champion match.</p>
			{/each}
		</RowList>
	</Panel>
</div>
