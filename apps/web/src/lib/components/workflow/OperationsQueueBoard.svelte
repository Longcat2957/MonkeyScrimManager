<script lang="ts">
	import { ArrowRight, ClipboardCheck, Gavel, ListFilter, Swords, UsersRound } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import type { OperationKind, OperationsQueueItem } from '$lib/fixtures/componentDevelopment';

	type QueueFilter = 'ALL' | OperationKind;

	interface Props {
		items: OperationsQueueItem[];
	}

	let { items }: Props = $props();
	let filter = $state<QueueFilter>('ALL');

	const filters: Array<{ value: QueueFilter; label: string }> = [
		{ value: 'ALL', label: '전체' },
		{ value: 'ENTRY', label: '편성' },
		{ value: 'PICK_BAN', label: '픽밴' },
		{ value: 'RESULT', label: '결과' },
		{ value: 'AUCTION', label: '경매' }
	];

	const kindMeta = {
		ENTRY: { label: '일반 모집', icon: UsersRound, iconClass: 'text-cyan-200' },
		PICK_BAN: { label: '일반 내전', icon: Swords, iconClass: 'text-indigo-200' },
		RESULT: { label: '결과 대기', icon: ClipboardCheck, iconClass: 'text-amber-200' },
		AUCTION: { label: '경매 내전', icon: Gavel, iconClass: 'text-emerald-200' }
	} as const;

	const visibleItems = $derived(
		filter === 'ALL' ? items : items.filter((item) => item.kind === filter)
	);
	const urgentCount = $derived(items.filter((item) => item.priority === 'urgent').length);

	function statusTone(kind: OperationKind): 'info' | 'warning' | 'success' {
		if (kind === 'ENTRY' || kind === 'RESULT') return 'warning';
		if (kind === 'AUCTION') return 'success';
		return 'info';
	}

	function actionLabel(kind: OperationKind): string {
		if (kind === 'ENTRY') return '편성 시작';
		if (kind === 'PICK_BAN') return '픽밴 열기';
		if (kind === 'RESULT') return '결과 입력';
		return '경매 열기';
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex flex-col gap-3 border-b border-[var(--color-app-border)] px-4 py-3 lg:flex-row lg:items-center lg:justify-between"
	>
		<div class="min-w-0">
			<div class="flex items-center gap-2">
				<h2 class="text-sm font-semibold text-slate-100">운영 대기열</h2>
				<span
					class="inline-flex h-6 min-w-6 items-center justify-center border border-amber-300/30 bg-amber-300/10 px-1.5 text-[11px] font-semibold tabular-nums text-amber-100"
					>{urgentCount}</span
				>
			</div>
			<p class="mt-1 text-xs text-slate-500">다음 입력이 필요한 작업만 표시합니다.</p>
		</div>

		<div
			class="flex max-w-full items-center overflow-x-auto border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
			role="tablist"
			aria-label="작업 종류"
		>
			<ListFilter size={13} class="mx-2 shrink-0 text-slate-600" aria-hidden="true" />
			{#each filters as item (item.value)}
				<button
					type="button"
					role="tab"
					aria-selected={filter === item.value}
					onclick={() => (filter = item.value)}
					class={`h-8 shrink-0 rounded-sm px-3 text-[11px] font-semibold ${
						filter === item.value
							? 'bg-cyan-400/10 text-cyan-100'
							: 'text-slate-500 hover:text-slate-200'
					}`}
				>
					{item.label}
				</button>
			{/each}
		</div>
	</header>

	<div
		class="hidden grid-cols-[minmax(220px,1fr)_minmax(240px,1.2fr)_90px_112px] border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
	>
		<span>작업</span><span>현재 상태</span><span>갱신</span><span></span>
	</div>

	<div class="divide-y divide-[var(--color-app-border-muted)]">
		{#each visibleItems as item (item.id)}
			{@const meta = kindMeta[item.kind]}
			<div
				class="grid gap-3 px-4 py-3 md:grid-cols-[minmax(220px,1fr)_minmax(240px,1.2fr)_90px_112px] md:items-center"
			>
				<div class="flex min-w-0 items-center gap-3">
					<div
						class="grid size-9 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)]"
					>
						<meta.icon size={16} class={meta.iconClass} aria-hidden="true" />
					</div>
					<div class="min-w-0">
						<strong class="block truncate text-sm text-slate-100">{item.title}</strong>
						<span class="mt-0.5 block text-[11px] text-slate-600">{meta.label}</span>
					</div>
				</div>

				<div class="min-w-0">
					<div class="flex min-w-0 items-center gap-2">
						<StatusPill tone={statusTone(item.kind)} label={item.status} />
						<p class="truncate text-xs text-slate-400">{item.detail}</p>
					</div>
				</div>

				<span class="text-xs tabular-nums text-slate-600">{item.updatedAt}</span>

				<a
					href={item.href}
					class="inline-flex h-9 items-center justify-center gap-1.5 whitespace-nowrap rounded-sm border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
				>
					{actionLabel(item.kind)}
					<ArrowRight size={14} aria-hidden="true" />
				</a>
			</div>
		{:else}
			<div class="grid min-h-32 place-items-center px-4 py-8 text-sm text-slate-600">
				해당 작업 없음
			</div>
		{/each}
	</div>
</section>
