<script lang="ts">
	import { Search } from 'lucide-svelte';
	import { resolve } from '$app/paths';
	import RiotRecordsView from '$lib/components/records/RiotRecordsView.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>{data.query ? `${data.query} 전적` : 'Riot 전적 검색'} | MonkeyBot</title>
	<meta
		name="description"
		content="Riot ID로 현재 시즌 챔피언 통계와 최근 경기 상세 기록을 조회합니다."
	/>
</svelte:head>

{#if data.records && data.lookup}
	<RiotRecordsView
		initialRecords={data.records}
		lookup={data.lookup}
		catalog={data.championCatalog}
	/>
{:else}
	<main class="mx-auto flex w-full max-w-5xl flex-1 items-start px-4 py-8 sm:px-6">
		<section class="w-full border border-[var(--color-app-border)] bg-[var(--color-app-surface)]">
			<header class="border-b border-[var(--color-app-border)] px-5 py-4">
				<h1 class="text-base font-semibold text-slate-100">Riot 전적 검색</h1>
				<p class="mt-1 text-xs text-slate-500">Riot ID를 입력해 현재 시즌 기록을 조회합니다.</p>
			</header>

			<form method="GET" action={resolve('/records')} class="flex gap-2 p-5">
				<label class="sr-only" for="riot-record-query">Riot ID</label>
				<input
					id="riot-record-query"
					name="q"
					value={data.query}
					placeholder="GameName#TagLine"
					autocomplete="off"
					class="h-10 min-w-0 flex-1 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-400/45"
				/>
				<button
					type="submit"
					class="inline-flex h-10 shrink-0 items-center gap-2 border border-cyan-400/35 bg-cyan-400/10 px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15"
				>
					<Search size={15} aria-hidden="true" />
					검색
				</button>
			</form>

			{#if data.loadError}
				<p class="border-t border-rose-400/20 bg-rose-400/5 px-5 py-4 text-xs text-rose-200">
					{data.loadError}
				</p>
			{/if}
		</section>
	</main>
{/if}
