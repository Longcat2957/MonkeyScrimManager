<script lang="ts">
	import { ArrowRight, Gavel, ListFilter, Swords, UsersRound } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import {
		operationActionLabelForRole,
		operationStageView,
		selectPrimaryOperation,
		type OperationStageView,
		type OperationViewItem
	} from '$lib/workflow/presentation';

	type OperationKind = OperationViewItem['kind'];
	type QueueFilter = 'ALL' | OperationKind;

	interface Props {
		items: OperationViewItem[];
		currentUserRole: 'MEMBER' | 'OPERATOR';
	}

	let { items, currentUserRole }: Props = $props();
	let filter = $state<QueueFilter>('ALL');

	const filters: Array<{ value: QueueFilter; label: string }> = [
		{ value: 'ALL', label: '전체' },
		{ value: 'ENTRY', label: '편성' },
		{ value: 'PICK_BAN', label: '픽밴' },
		{ value: 'AUCTION', label: '경매' }
	];

	const kindMeta = {
		ENTRY: { label: '일반 모집', icon: UsersRound, iconClass: 'text-cyan-200' },
		PICK_BAN: { label: '일반 내전', icon: Swords, iconClass: 'text-indigo-200' },
		AUCTION: { label: '경매 내전', icon: Gavel, iconClass: 'text-emerald-200' }
	} as const;

	const primaryItem = $derived(selectPrimaryOperation(items));
	const primaryStages = $derived(primaryItem ? operationStageView(primaryItem) : []);
	const remainingItems = $derived(
		primaryItem ? items.filter((item) => item.id !== primaryItem.id) : []
	);
	const visibleItems = $derived(
		filter === 'ALL' ? remainingItems : remainingItems.filter((item) => item.kind === filter)
	);
	const urgentCount = $derived(remainingItems.filter((item) => item.priority === 'urgent').length);

	function actionLabel(item: OperationViewItem): string {
		return operationActionLabelForRole(item, currentUserRole);
	}

	function markerClass(state: OperationStageView['state']): string {
		if (state === 'current')
			return 'size-3 border-cyan-200 bg-cyan-300 shadow-[0_0_0_4px_rgb(34_211_238_/_0.12)]';
		if (state === 'complete') return 'size-2 border-cyan-400/60 bg-cyan-400/70';
		return 'size-2 border-slate-700 bg-slate-900';
	}

	function labelClass(state: OperationStageView['state']): string {
		if (state === 'current') return 'text-cyan-100';
		if (state === 'complete') return 'text-slate-400';
		return 'text-slate-700';
	}
</script>

{#if primaryItem}
	{@const primaryMeta = kindMeta[primaryItem.kind]}
	<section
		class="overflow-hidden border border-cyan-300/25 bg-[linear-gradient(135deg,rgb(8_145_178_/_0.075),transparent_52%),var(--color-app-surface)]"
		aria-labelledby="primary-operation-title"
	>
		<header
			class="flex flex-col gap-3 border-b border-cyan-300/15 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
		>
			<div class="flex min-w-0 items-center gap-3">
				<div
					class="grid size-9 shrink-0 place-items-center border border-cyan-300/20 bg-cyan-400/[0.065]"
				>
					<primaryMeta.icon size={16} class={primaryMeta.iconClass} aria-hidden="true" />
				</div>
				<div class="min-w-0">
					<p class="text-[10px] font-semibold tracking-[0.14em] text-cyan-300 uppercase">
						{currentUserRole === 'OPERATOR' ? '지금 처리할 작업' : '현재 진행 상황'}
					</p>
					<p class="mt-0.5 text-xs text-slate-500">{primaryMeta.label}</p>
				</div>
			</div>
			<div class="flex items-center gap-2">
				{#if primaryItem.priority === 'urgent'}
					<span
						class="inline-flex h-6 items-center border border-amber-300/30 bg-amber-300/10 px-2 text-[10px] font-semibold text-amber-100"
						>우선 처리</span
					>
				{/if}
				<StatusPill tone={primaryItem.tone} label={primaryItem.status} />
			</div>
		</header>

		<div class="px-4 py-5 sm:px-5">
			<div class="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
				<div class="min-w-0">
					<h2 id="primary-operation-title" class="truncate text-xl font-semibold text-slate-50">
						{primaryItem.title}
					</h2>
					<p class="mt-1 text-sm text-slate-400">{primaryItem.detail}</p>
					<p class="mt-2 text-xs tabular-nums text-slate-600">{primaryItem.updatedAt} 갱신</p>
				</div>
				<a
					href={primaryItem.href}
					class="inline-flex h-10 w-full shrink-0 items-center justify-center gap-2 border border-cyan-300/35 bg-cyan-400/10 px-4 text-xs font-semibold text-cyan-50 hover:bg-cyan-400/15 sm:w-auto"
				>
					{actionLabel(primaryItem)}
					<ArrowRight size={15} aria-hidden="true" />
				</a>
			</div>

			<div class="mt-5 overflow-x-auto pb-1">
				<ol class="flex min-w-[32rem]" aria-label={`${primaryItem.title} 진행 단계`}>
					{#each primaryStages as stage, index (stage.label)}
						<li class="flex min-w-0 flex-1 flex-col items-center">
							<div class="flex h-3 w-full items-center" aria-hidden="true">
								<span
									class={`h-px flex-1 ${
										index === 0
											? 'bg-transparent'
											: stage.state !== 'upcoming'
												? 'bg-cyan-400/45'
												: 'bg-slate-800'
									}`}
								></span>
								<span class={`shrink-0 rounded-full border ${markerClass(stage.state)}`}></span>
								<span
									class={`h-px flex-1 ${
										index === primaryStages.length - 1
											? 'bg-transparent'
											: stage.state === 'complete'
												? 'bg-cyan-400/45'
												: 'bg-slate-800'
									}`}
								></span>
							</div>
							<span
								class={`mt-2 text-[10px] font-semibold ${labelClass(stage.state)}`}
								aria-current={stage.state === 'current' ? 'step' : undefined}>{stage.label}</span
							>
						</li>
					{/each}
				</ol>
			</div>
		</div>
	</section>
{/if}

{#if remainingItems.length > 0}
	<section
		class="mt-4 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex flex-col gap-3 border-b border-[var(--color-app-border)] px-4 py-3 lg:flex-row lg:items-center lg:justify-between"
		>
			<div class="min-w-0">
				<div class="flex items-center gap-2">
					<h2 class="text-sm font-semibold text-slate-100">
						{currentUserRole === 'OPERATOR' ? '남은 운영 작업' : '다른 진행 작업'}
					</h2>
					{#if urgentCount > 0}
						<span
							class="inline-flex h-6 items-center justify-center border border-amber-300/30 bg-amber-300/10 px-2 text-[11px] font-semibold tabular-nums text-amber-100"
							aria-label={`남은 긴급 작업 ${urgentCount}건`}>긴급 {urgentCount}</span
						>
					{/if}
				</div>
				<p class="mt-1 text-xs text-slate-500">우선 작업을 제외한 진행 항목입니다.</p>
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
							<StatusPill tone={item.tone} label={item.status} />
							<p class="truncate text-xs text-slate-400">{item.detail}</p>
						</div>
					</div>

					<span class="text-xs tabular-nums text-slate-600">{item.updatedAt}</span>

					<a
						href={item.href}
						class="inline-flex h-9 items-center justify-center gap-1.5 whitespace-nowrap rounded-sm border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
					>
						{actionLabel(item)}
						<ArrowRight size={14} aria-hidden="true" />
					</a>
				</div>
			{:else}
				<div class="grid min-h-32 place-items-center px-4 py-8 text-sm text-slate-600">
					해당 종류의 추가 작업 없음
				</div>
			{/each}
		</div>
	</section>
{/if}
