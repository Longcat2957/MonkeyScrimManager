<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import {
		ArrowRight,
		CheckCircle2,
		LoaderCircle,
		Plus,
		Swords,
		Trophy,
		UsersRound,
		X
	} from 'lucide-svelte';
	import { createRecruitment, recruitmentCreateErrorText } from '$lib/api/recruitmentCreate';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import {
		formatSeriesRules,
		seriesBestOfValues,
		type SeriesBestOf
	} from '$lib/workflow/seriesRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const activeWork = $derived(data.view.activeWork);
	const completedSeries = $derived(data.view.completedSeries);
	let creatingRecruitment = $state(false);
	let createError = $state('');
	let createIdempotencyKey = $state<string | null>(null);
	let createDialogOpen = $state(false);
	let createBestOf = $state<SeriesBestOf>(3);
	let createHardFearless = $state(false);

	function openCreateDialog() {
		if (creatingRecruitment) return;
		createError = '';
		createIdempotencyKey = null;
		createBestOf = 3;
		createHardFearless = false;
		createDialogOpen = true;
	}

	function selectBestOf(bestOf: SeriesBestOf) {
		if (creatingRecruitment || createBestOf === bestOf) return;
		createBestOf = bestOf;
		createIdempotencyKey = null;
	}

	function toggleHardFearless() {
		if (creatingRecruitment) return;
		createHardFearless = !createHardFearless;
		createIdempotencyKey = null;
	}

	async function createNewRecruitment() {
		if (creatingRecruitment) return;
		const idempotencyKey = (createIdempotencyKey ??= globalThis.crypto.randomUUID());
		creatingRecruitment = true;
		createError = '';
		try {
			const created = await createRecruitment(
				data.guildId,
				{ bestOf: createBestOf, hardFearless: createHardFearless },
				idempotencyKey
			);
			createIdempotencyKey = null;
			createDialogOpen = false;
			await goto(
				resolve('/g/[guildId]/recruitments/[id]', {
					guildId: data.guildId,
					id: created.recruitment.id
				})
			);
		} catch (cause) {
			createError = recruitmentCreateErrorText(cause);
		} finally {
			creatingRecruitment = false;
		}
	}
</script>

<svelte:head>
	<title>내전 운영 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader kicker="일반 내전" title="내전 운영" description="모집 마감부터 완료 내전까지">
		{#snippet actions()}
			<button
				type="button"
				disabled={creatingRecruitment}
				onclick={openCreateDialog}
				class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35"
			>
				{#if creatingRecruitment}
					<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
				{:else}
					<Plus size={14} aria-hidden="true" />
				{/if}
				{creatingRecruitment ? '생성 중' : '내전 모집'}
			</button>
		{/snippet}
	</WorkspaceHeader>

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<Swords size={16} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">진행 중</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{activeWork.length}건</span>
		</header>
		<div
			class="hidden grid-cols-[minmax(220px,1fr)_90px_minmax(200px,1fr)_90px_116px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
		>
			<span>시리즈</span><span>포맷</span><span>현재 상태</span><span>갱신</span><span></span>
		</div>
		<div class="divide-y divide-[var(--color-app-border-muted)]">
			{#each activeWork as item (item.key)}
				{@const ItemIcon = item.kind === 'RECRUITMENT' ? UsersRound : Swords}
				<div
					class="grid gap-3 px-4 py-3 md:grid-cols-[minmax(220px,1fr)_90px_minmax(200px,1fr)_90px_116px] md:items-center"
				>
					<div class="flex min-w-0 items-center gap-3">
						<span
							class="grid size-9 shrink-0 place-items-center border border-cyan-400/25 bg-cyan-400/8 text-cyan-200"
						>
							<ItemIcon size={16} aria-hidden="true" />
						</span>
						<div class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{item.title}</strong>
							<small class="text-[10px] text-slate-600">#{item.displayNumber}</small>
						</div>
					</div>
					<span class="text-xs font-semibold text-slate-400">{item.format}</span>
					<div class="flex min-w-0 items-center gap-2">
						<StatusPill label={item.status} tone="warning" />
						<span class="truncate text-xs text-slate-500">{item.detail}</span>
					</div>
					<span class="text-xs text-slate-600">{item.updatedAt}</span>
					<a
						href={item.href}
						class="inline-flex h-9 items-center justify-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
					>
						{item.actionLabel}
						<ArrowRight size={14} aria-hidden="true" />
					</a>
				</div>
			{:else}
				<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
					진행 중인 내전이 없습니다.
				</div>
			{/each}
		</div>
	</section>

	<section
		id="completed"
		class="scroll-mt-20 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<CheckCircle2 size={16} class="shrink-0 text-emerald-300" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">완료된 내전</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{completedSeries.length}건</span>
		</header>

		<div class="divide-y divide-[var(--color-app-border-muted)] md:hidden">
			{#each completedSeries as series (series.key)}
				<a href={series.href} class="grid gap-3 p-3 hover:bg-[var(--color-app-hover)]">
					<div class="flex min-w-0 items-start justify-between gap-3">
						<span class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{series.name}</strong>
							<small class="text-[10px] text-slate-600"
								>#{series.displayNumber} · {series.playedAt}</small
							>
						</span>
						<ArrowRight size={15} class="shrink-0 text-slate-600" aria-hidden="true" />
					</div>
					<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
						<div class="bg-[var(--color-app-surface)] px-2 py-2">
							<span class="block text-[9px] font-semibold text-slate-600">포맷</span>
							<strong class="mt-0.5 block text-xs text-slate-300">{series.format}</strong>
						</div>
						<div class="bg-[var(--color-app-surface)] px-2 py-2 text-center">
							<span class="block text-[9px] font-semibold text-slate-600">스코어</span>
							<strong class="mt-0.5 block text-sm tabular-nums text-slate-100"
								>{series.score}</strong
							>
						</div>
						<div class="bg-[var(--color-app-surface)] px-2 py-2">
							<span class="block text-[9px] font-semibold text-slate-600">결과</span>
							<strong
								class={`mt-0.5 flex items-center gap-1 text-xs ${
									series.outcome === 'TEAM_1' || series.outcome === 'TEAM_2'
										? 'text-amber-200'
										: 'text-slate-300'
								}`}
							>
								{#if series.outcome === 'TEAM_1' || series.outcome === 'TEAM_2'}
									<Trophy size={12} aria-hidden="true" />
								{/if}
								{series.winner}
							</strong>
						</div>
					</div>
				</a>
			{:else}
				<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
					완료된 내전이 없습니다.
				</div>
			{/each}
		</div>

		<div class="hidden md:block">
			<div
				class="grid grid-cols-[minmax(240px,1fr)_90px_110px_110px_90px_40px] items-center border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>시리즈</span><span>포맷</span><span class="text-center">스코어</span><span>결과</span
				><span>완료</span><span></span>
			</div>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each completedSeries as series (series.key)}
					<a
						href={series.href}
						class="grid min-h-16 grid-cols-[minmax(240px,1fr)_90px_110px_110px_90px_40px] items-center px-4 hover:bg-[var(--color-app-hover)]"
					>
						<span class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{series.name}</strong>
							<small class="text-[10px] text-slate-600">#{series.displayNumber}</small>
						</span>
						<span class="text-xs text-slate-500">{series.format}</span>
						<strong class="text-center text-base tabular-nums text-slate-100">{series.score}</strong
						>
						<span
							class={`inline-flex items-center gap-1.5 text-xs font-semibold ${
								series.outcome === 'TEAM_1' || series.outcome === 'TEAM_2'
									? 'text-amber-200'
									: 'text-slate-300'
							}`}
						>
							{#if series.outcome === 'TEAM_1' || series.outcome === 'TEAM_2'}
								<Trophy size={13} aria-hidden="true" />
							{/if}
							{series.winner}
						</span>
						<span class="text-xs text-slate-500">{series.playedAt}</span>
						<ArrowRight size={15} class="text-slate-600" aria-hidden="true" />
					</a>
				{:else}
					<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
						완료된 내전이 없습니다.
					</div>
				{/each}
			</div>
		</div>
	</section>
</div>

<Dialog.Root bind:open={createDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">내전 모집</Dialog.Title>
				<Dialog.Close
					disabled={creatingRecruitment}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="내전 모집 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>

			<div class="grid gap-5 p-4">
				<div class="grid gap-2">
					<span class="text-[11px] font-semibold text-slate-500">시리즈 형식</span>
					<div
						class="grid grid-cols-3 border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
						role="radiogroup"
						aria-label="시리즈 형식"
					>
						{#each seriesBestOfValues as bestOf (bestOf)}
							<button
								type="button"
								role="radio"
								aria-checked={createBestOf === bestOf}
								disabled={creatingRecruitment}
								onclick={() => selectBestOf(bestOf)}
								class={`h-9 text-xs font-semibold disabled:opacity-35 ${
									createBestOf === bestOf
										? 'bg-cyan-400/15 text-cyan-100'
										: 'text-slate-500 hover:text-slate-200'
								}`}
							>
								BO{bestOf}
							</button>
						{/each}
					</div>
				</div>

				<button
					type="button"
					role="switch"
					aria-checked={createHardFearless}
					disabled={creatingRecruitment}
					onclick={toggleHardFearless}
					class="flex min-h-12 items-center justify-between gap-4 border border-[var(--color-app-border)] px-3 text-left enabled:hover:bg-[var(--color-app-hover)] disabled:opacity-35"
				>
					<span class="grid gap-0.5">
						<strong class="text-xs font-semibold text-slate-200">하드 피어리스</strong>
						<span class="text-[11px] text-slate-500">이전 세트의 픽은 다시 선택할 수 없습니다.</span
						>
					</span>
					<span
						class={`relative h-5 w-9 shrink-0 border transition-colors ${
							createHardFearless
								? 'border-cyan-300/65 bg-cyan-400/35'
								: 'border-slate-600 bg-slate-800'
						}`}
						aria-hidden="true"
					>
						<span
							class={`absolute top-0.5 size-3.5 bg-slate-100 transition-transform ${
								createHardFearless ? 'translate-x-4' : 'translate-x-0.5'
							}`}
						></span>
					</span>
				</button>

				<p class="text-xs font-semibold text-slate-400">
					{formatSeriesRules(createBestOf, createHardFearless)}
				</p>
				{#if createError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{createError}
					</p>
				{/if}
			</div>

			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={creatingRecruitment}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={creatingRecruitment}
					onclick={createNewRecruitment}
					class="inline-flex h-9 items-center gap-1.5 border border-cyan-300/35 bg-cyan-400/15 px-3 text-xs font-semibold text-cyan-50 disabled:opacity-35"
				>
					{#if creatingRecruitment}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 생성 중
					{:else}
						<Plus size={14} aria-hidden="true" /> 모집 만들기
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
