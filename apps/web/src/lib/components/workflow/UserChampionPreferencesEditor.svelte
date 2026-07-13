<script lang="ts">
	import { ArrowDown, ArrowUp, Check, Plus, RotateCcw, Save, Search, X } from 'lucide-svelte';
	import { updateChampionPreferences } from '$lib/api/championPreferences';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import PositionIcon from '$lib/components/workflow/PositionIcon.svelte';
	import type { ChampionPreferenceLane, ChampionPreferences } from '$lib/contracts/workflow';
	import {
		searchChampions,
		type ChampionCatalog,
		type ChampionCatalogEntry
	} from '$lib/datadragon';

	interface Props {
		catalog: ChampionCatalog;
		initialPreferences: ChampionPreferences;
		maxPreferences?: number;
	}

	const laneOrder: ChampionPreferenceLane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const laneLabel: Record<ChampionPreferenceLane, string> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '원딜',
		SUPPORT: '서폿'
	};

	let { catalog, initialPreferences, maxPreferences = 10 }: Props = $props();
	let selectedLane = $state<ChampionPreferenceLane>('MID');
	let query = $state('');
	let savedPreferences = $derived<ChampionPreferences>(clonePreferences(initialPreferences));
	let draftPreferences = $derived<ChampionPreferences>(clonePreferences(initialPreferences));
	let savedNotice = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	const selectedIds = $derived(draftPreferences[selectedLane]);
	const selectedChampions = $derived(
		selectedIds
			.map((id) => catalog.byId[id])
			.filter((champion): champion is ChampionCatalogEntry => Boolean(champion))
	);
	const searchResults = $derived(
		searchChampions(catalog, query, { limit: catalog.champions.length })
	);
	const dirty = $derived(selectedIds.join('|') !== savedPreferences[selectedLane].join('|'));

	function clonePreferences(source: ChampionPreferences): ChampionPreferences {
		return Object.fromEntries(
			laneOrder.map((lane) => [lane, [...source[lane]]])
		) as ChampionPreferences;
	}

	function updateSelected(next: string[]) {
		if (saving) return;
		draftPreferences = { ...draftPreferences, [selectedLane]: next };
		savedNotice = false;
		error = null;
	}

	function selectLane(lane: ChampionPreferenceLane) {
		if (saving) return;
		selectedLane = lane;
		query = '';
		savedNotice = false;
		error = null;
	}

	function addChampion(championId: string) {
		if (selectedIds.includes(championId) || selectedIds.length >= maxPreferences) return;
		updateSelected([...selectedIds, championId]);
	}

	function removeChampion(championId: string) {
		updateSelected(selectedIds.filter((id) => id !== championId));
	}

	function moveChampion(index: number, direction: -1 | 1) {
		const target = index + direction;
		if (target < 0 || target >= selectedIds.length) return;

		const next = [...selectedIds];
		[next[index], next[target]] = [next[target], next[index]];
		updateSelected(next);
	}

	function resetLane() {
		if (saving) return;
		updateSelected([...savedPreferences[selectedLane]]);
	}

	async function saveLane() {
		if (!dirty || saving) return;
		const lane = selectedLane;
		const championIds = [...selectedIds];
		saving = true;
		savedNotice = false;
		error = null;
		try {
			const preferences = await updateChampionPreferences(lane, championIds);
			savedPreferences = clonePreferences(preferences);
			draftPreferences = clonePreferences(preferences);
			savedNotice = true;
		} catch (saveError) {
			error =
				saveError instanceof Error ? saveError.message : '선호 챔피언 저장 중 오류가 발생했습니다.';
		} finally {
			saving = false;
		}
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<PositionIcon role={selectedLane} size="md" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">선호 챔피언</h2>
				<p class="text-xs text-slate-500">
					{laneLabel[selectedLane]} · {selectedIds.length}/{maxPreferences}
				</p>
			</div>
		</div>
		<div class="flex items-center gap-2">
			{#if error}
				<span class="text-xs font-semibold text-rose-300" aria-live="assertive">{error}</span>
			{/if}
			{#if savedNotice}
				<span
					class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300"
					aria-live="polite"
				>
					<Check size={14} aria-hidden="true" /> 저장됨
				</span>
			{/if}
			<button
				type="button"
				class="grid size-8 place-items-center rounded-sm border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-400 enabled:hover:text-slate-100 disabled:opacity-35"
				disabled={!dirty || saving}
				aria-label="변경 취소"
				title="변경 취소"
				onclick={resetLane}
			>
				<RotateCcw size={15} aria-hidden="true" />
			</button>
			<button
				type="button"
				class="inline-flex h-8 items-center gap-1.5 rounded-sm border border-cyan-400/45 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 enabled:hover:bg-cyan-400/15 disabled:opacity-35"
				disabled={!dirty || saving}
				onclick={saveLane}
			>
				<Save size={14} aria-hidden="true" />
				{saving ? '저장 중...' : '저장'}
			</button>
		</div>
	</header>

	<div class="flex min-w-0 overflow-x-auto border-b border-[var(--color-app-border)] px-3 pt-2">
		{#each laneOrder as lane (lane)}
			<button
				type="button"
				class={`flex h-9 min-w-20 items-center justify-center gap-1.5 border-b-2 px-3 text-xs font-semibold ${
					selectedLane === lane
						? 'border-cyan-300 text-cyan-100'
						: 'border-transparent text-slate-500 hover:text-slate-300'
				} disabled:opacity-35`}
				disabled={saving}
				onclick={() => selectLane(lane)}
			>
				<PositionIcon role={lane} size="sm" />
				{laneLabel[lane]}
			</button>
		{/each}
	</div>

	<div class="grid min-h-[430px] lg:grid-cols-[minmax(0,1fr)_390px]">
		<div class="min-w-0 lg:border-r lg:border-[var(--color-app-border)]">
			<div
				class="grid grid-cols-[42px_44px_minmax(0,1fr)_116px] items-center border-b border-[var(--color-app-border-muted)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>순서</span><span></span><span>챔피언</span><span class="text-right">관리</span>
			</div>
			<div>
				{#each selectedChampions as champion, index (champion.id)}
					<div
						class="grid min-h-[58px] grid-cols-[42px_44px_minmax(0,1fr)_116px] items-center border-b border-[var(--color-app-border-muted)] px-4 py-2"
					>
						<span class="text-xs font-semibold tabular-nums text-slate-500">{index + 1}</span>
						<ChampionFace {champion} version={catalog.version} size="md" />
						<div class="min-w-0 px-2">
							<strong class="block truncate text-sm text-slate-100">{champion.name}</strong>
							<span class="block truncate text-[10px] text-slate-600">{champion.id}</span>
						</div>
						<div class="flex justify-end gap-1">
							<button
								type="button"
								class="grid size-8 place-items-center rounded-sm text-slate-500 hover:bg-[var(--color-app-hover)] hover:text-slate-200 disabled:opacity-25"
								disabled={saving || index === 0}
								aria-label={`${champion.name} 위로 이동`}
								title="위로 이동"
								onclick={() => moveChampion(index, -1)}
							>
								<ArrowUp size={15} aria-hidden="true" />
							</button>
							<button
								type="button"
								class="grid size-8 place-items-center rounded-sm text-slate-500 hover:bg-[var(--color-app-hover)] hover:text-slate-200 disabled:opacity-25"
								disabled={saving || index === selectedChampions.length - 1}
								aria-label={`${champion.name} 아래로 이동`}
								title="아래로 이동"
								onclick={() => moveChampion(index, 1)}
							>
								<ArrowDown size={15} aria-hidden="true" />
							</button>
							<button
								type="button"
								class="grid size-8 place-items-center rounded-sm text-slate-500 hover:bg-rose-400/10 hover:text-rose-300"
								disabled={saving}
								aria-label={`${champion.name} 삭제`}
								title="삭제"
								onclick={() => removeChampion(champion.id)}
							>
								<X size={15} aria-hidden="true" />
							</button>
						</div>
					</div>
				{:else}
					<p class="px-4 py-10 text-center text-sm text-slate-600">등록된 챔피언 없음</p>
				{/each}
			</div>
		</div>

		<div class="min-w-0 bg-[var(--color-app-bg-soft)] p-3">
			<label
				class="flex h-10 items-center gap-2 rounded-sm border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 focus-within:border-cyan-400/50"
			>
				<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
				<input
					bind:value={query}
					class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
					placeholder="챔피언 검색"
					autocomplete="off"
					disabled={saving}
				/>
			</label>

			<div class="mt-3 grid max-h-[358px] grid-cols-2 gap-1 overflow-y-auto pr-1">
				{#each searchResults as champion (champion.id)}
					{@const isSelected = selectedIds.includes(champion.id)}
					<button
						type="button"
						class={`grid min-h-11 grid-cols-[32px_minmax(0,1fr)_24px] items-center gap-2 rounded-sm px-2 text-left ${
							isSelected
								? 'bg-cyan-400/5 text-slate-300'
								: 'text-slate-300 hover:bg-[var(--color-app-hover)] disabled:opacity-35'
						}`}
						disabled={saving || isSelected || selectedIds.length >= maxPreferences}
						onclick={() => addChampion(champion.id)}
					>
						<ChampionFace {champion} version={catalog.version} size="sm" />
						<span class="truncate text-xs font-semibold">{champion.name}</span>
						{#if isSelected}
							<Check size={14} class="text-cyan-300" aria-hidden="true" />
						{:else}
							<Plus size={14} class="text-slate-600" aria-hidden="true" />
						{/if}
					</button>
				{:else}
					<p class="col-span-2 px-3 py-10 text-center text-sm text-slate-600">검색 결과 없음</p>
				{/each}
			</div>
		</div>
	</div>
</section>
