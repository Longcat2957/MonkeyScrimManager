<script lang="ts">
	import { ChevronDown, FileClock, Search, ShieldCheck } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import { mockGuildAuditEntries, type GuildAuditEntry } from '$lib/fixtures/guildAdmin';

	type CategoryFilter = '전체' | GuildAuditEntry['category'];

	let query = $state('');
	let categoryFilter = $state<CategoryFilter>('전체');
	let operatorFilter = $state('전체');
	let expandedEntryId = $state<string | null>(null);

	const categories: CategoryFilter[] = ['전체', '멤버', 'MMR', '내전', '경매', '설정'];
	const operators = ['전체', ...new Set(mockGuildAuditEntries.map((entry) => entry.operator))];
	const filteredEntries = $derived(
		mockGuildAuditEntries.filter((entry) => {
			const normalizedQuery = query.trim().toLocaleLowerCase();
			const matchesQuery =
				!normalizedQuery ||
				entry.action.toLocaleLowerCase().includes(normalizedQuery) ||
				entry.target.toLocaleLowerCase().includes(normalizedQuery);
			return (
				matchesQuery &&
				(categoryFilter === '전체' || entry.category === categoryFilter) &&
				(operatorFilter === '전체' || entry.operator === operatorFilter)
			);
		})
	);

	function toggleEntry(entryId: string) {
		expandedEntryId = expandedEntryId === entryId ? null : entryId;
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 items-center gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<FileClock size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
		<div class="min-w-0">
			<h2 class="truncate text-sm font-semibold text-slate-50">감사 로그</h2>
			<p class="text-xs text-slate-500">길드 관리 작업과 상태 변경 기록</p>
		</div>
	</header>

	<div
		class="grid gap-2 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-3 md:grid-cols-[minmax(0,1fr)_140px_140px]"
	>
		<label
			class="flex h-10 min-w-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3"
		>
			<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
			<input
				bind:value={query}
				placeholder="작업 또는 대상 검색"
				aria-label="감사 로그 검색"
				class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
			/>
		</label>
		<select
			bind:value={categoryFilter}
			aria-label="감사 로그 분류"
			class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
		>
			{#each categories as category (category)}
				<option value={category}>{category === '전체' ? '전체 작업' : category}</option>
			{/each}
		</select>
		<select
			bind:value={operatorFilter}
			aria-label="감사 로그 실행자"
			class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
		>
			{#each operators as operator (operator)}
				<option value={operator}>{operator === '전체' ? '전체 실행자' : operator}</option>
			{/each}
		</select>
	</div>

	<div class="overflow-x-auto">
		<div class="min-w-[850px]">
			<div
				class="grid grid-cols-[100px_170px_minmax(220px,1fr)_100px_100px_44px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>분류</span><span>작업</span><span>대상</span><span>실행자</span><span>시각</span><span
				></span>
			</div>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each filteredEntries as entry (entry.id)}
					<div>
						<button
							type="button"
							onclick={() => toggleEntry(entry.id)}
							aria-expanded={expandedEntryId === entry.id}
							class="grid w-full grid-cols-[100px_170px_minmax(220px,1fr)_100px_100px_44px] items-center px-4 py-3 text-left hover:bg-[var(--color-app-hover)]"
						>
							<StatusPill label={entry.category} tone="neutral" />
							<div class="flex min-w-0 items-center gap-2">
								<ShieldCheck size={14} class="shrink-0 text-cyan-200" aria-hidden="true" />
								<strong class="truncate text-xs text-slate-200">{entry.action}</strong>
							</div>
							<span class="truncate text-xs text-slate-400">{entry.target}</span>
							<span class="text-xs text-slate-500">{entry.operator}</span>
							<span class="text-xs text-slate-600">{entry.createdAt}</span>
							<ChevronDown
								size={15}
								class={`justify-self-end text-slate-500 transition ${expandedEntryId === entry.id ? 'rotate-180' : ''}`}
								aria-hidden="true"
							/>
						</button>
						{#if expandedEntryId === entry.id}
							<div
								class="grid gap-3 border-t border-[var(--color-app-border-muted)] bg-slate-950/20 px-4 py-3 sm:grid-cols-[100px_minmax(0,1fr)]"
							>
								<StatusPill
									label={entry.result}
									tone={entry.result === '복구됨' ? 'warning' : 'success'}
								/>
								<div>
									<p class="text-xs leading-5 text-slate-300">{entry.detail}</p>
									<small class="mt-1 block font-mono text-[10px] text-slate-600">{entry.id}</small>
								</div>
							</div>
						{/if}
					</div>
				{:else}
					<div class="px-4 py-10 text-center text-sm text-slate-600">일치하는 로그 없음</div>
				{/each}
			</div>
		</div>
	</div>
</section>
