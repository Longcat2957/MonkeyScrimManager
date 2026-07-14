<script lang="ts">
	import { goto } from '$app/navigation';
	import { Dialog } from 'bits-ui';
	import { LoaderCircle, RefreshCw, Trash2, Undo2, X } from 'lucide-svelte';
	import {
		deleteSeries,
		isSeriesPickBanRevisionConflict,
		revertLastSeriesPickBanGame,
		seriesPickBanErrorText
	} from '$lib/api/seriesPickBan';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import { fetchSeriesResult, seriesResultErrorText } from '$lib/api/seriesResult';
	import SeriesResultLedgerBoard from '$lib/components/workflow/SeriesResultLedgerBoard.svelte';
	import type { SeriesResultSnapshot } from '$lib/contracts/seriesResult';
	import type { PageProps } from './$types';

	type ResultMutation = 'revertLastGame' | 'delete';
	type ResultRoute = {
		key: string;
		guildId: string;
		seriesId: string;
	};

	let { data }: PageProps = $props();
	let snapshot = $state<SeriesResultSnapshot | null>(null);
	let loading = $state(true);
	let loadError = $state('');
	let mutation = $state<ResultMutation | null>(null);
	let mutationError = $state('');
	let mutationNotice = $state('');
	let revertLastGameDialogOpen = $state(false);
	let revertLastGameError = $state('');
	let deleteDialogOpen = $state(false);
	let deleteError = $state('');
	let activeRouteKey = $state<string | null>(null);
	const idempotencyKeys = createOperationIdempotencyKeys();
	const isMutating = $derived(mutation !== null);
	const canRevertLastGame = $derived(Boolean(snapshot?.capabilities.canRevertLastGame));
	const canDelete = $derived(Boolean(snapshot?.capabilities.canDelete));

	$effect(() => {
		const route = currentResultRoute();
		if (route.key === activeRouteKey) return;

		activeRouteKey = route.key;
		resetRouteState();
		void loadResult(route);
	});

	function currentResultRoute(): ResultRoute {
		return {
			key: `${data.guildId}:${data.seriesId}`,
			guildId: data.guildId,
			seriesId: data.seriesId
		};
	}

	function isCurrentRoute(route: ResultRoute): boolean {
		return activeRouteKey === route.key;
	}

	function resetRouteState() {
		snapshot = null;
		loading = true;
		loadError = '';
		mutation = null;
		mutationError = '';
		mutationNotice = '';
		revertLastGameDialogOpen = false;
		revertLastGameError = '';
		deleteDialogOpen = false;
		deleteError = '';
		idempotencyKeys.invalidate('series-result-revert-last-game');
		idempotencyKeys.invalidate('series-result-delete');
	}

	async function loadResult(route = currentResultRoute()) {
		if (!isCurrentRoute(route)) return;
		loading = true;
		loadError = '';
		try {
			const nextSnapshot = await fetchSeriesResult(route.guildId, route.seriesId);
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			loadError = seriesResultErrorText(cause);
		} finally {
			if (isCurrentRoute(route)) loading = false;
		}
	}

	function invalidateIdempotencyKey(operation: ResultMutation) {
		if (operation === 'revertLastGame') {
			idempotencyKeys.invalidate('series-result-revert-last-game');
		}
		if (operation === 'delete') {
			idempotencyKeys.invalidate('series-result-delete');
		}
	}

	async function recoverAfterRevisionConflict(
		cause: unknown,
		operation: ResultMutation,
		route: ResultRoute
	) {
		invalidateIdempotencyKey(operation);
		mutationError = '';
		mutationNotice = '';
		try {
			const nextSnapshot = await fetchSeriesResult(route.guildId, route.seriesId);
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
			loadError = '';
			mutationNotice = `${seriesPickBanErrorText(cause)} 서버 최신 상태를 불러왔습니다. 변경된 결과를 확인한 뒤 다시 시도해 주세요.`;
		} catch (refreshCause) {
			if (!isCurrentRoute(route)) return;
			mutationError = `${seriesPickBanErrorText(cause)} 최신 상태를 불러오지 못했습니다. ${seriesResultErrorText(refreshCause)}`;
		}
	}

	function openRevertLastGameDialog() {
		if (!snapshot || !canRevertLastGame || isMutating) return;
		deleteDialogOpen = false;
		mutationError = '';
		mutationNotice = '';
		revertLastGameError = '';
		revertLastGameDialogOpen = true;
	}

	function openDeleteDialog() {
		if (!snapshot || !canDelete || isMutating) return;
		revertLastGameDialogOpen = false;
		mutationError = '';
		mutationNotice = '';
		deleteError = '';
		deleteDialogOpen = true;
	}

	async function confirmRevertLastGame() {
		const route = currentResultRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canRevertLastGame || isMutating) return;

		const operation = { expectedRevision: currentSnapshot.series.revision };
		const idempotencyKey = idempotencyKeys.keyFor('series-result-revert-last-game', operation);
		mutation = 'revertLastGame';
		mutationError = '';
		mutationNotice = '';
		revertLastGameError = '';
		try {
			const result = await revertLastSeriesPickBanGame(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-result-revert-last-game');
			revertLastGameDialogOpen = false;
			await goto(result.nextHref);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isSeriesPickBanRevisionConflict(cause)) {
				revertLastGameDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'revertLastGame', route);
			} else {
				revertLastGameError = seriesPickBanErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) mutation = null;
		}
	}

	async function confirmDelete() {
		const route = currentResultRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canDelete || isMutating) return;

		const operation = { expectedRevision: currentSnapshot.series.revision };
		const idempotencyKey = idempotencyKeys.keyFor('series-result-delete', operation);
		mutation = 'delete';
		mutationError = '';
		mutationNotice = '';
		deleteError = '';
		try {
			const result = await deleteSeries(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-result-delete');
			deleteDialogOpen = false;
			await goto(result.nextHref);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isSeriesPickBanRevisionConflict(cause)) {
				deleteDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'delete', route);
			} else {
				deleteError = seriesPickBanErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) mutation = null;
		}
	}
</script>

<svelte:head>
	<title>시리즈 결과 | MonkeyBot</title>
</svelte:head>

{#if loading && !snapshot}
	<section
		class="flex min-h-56 items-center justify-center border border-[var(--color-app-border)] bg-[var(--color-app-surface)] px-4 text-center"
		aria-busy="true"
	>
		<div class="grid justify-items-center gap-2 text-sm font-semibold text-slate-400">
			<LoaderCircle size={20} class="animate-spin text-cyan-200" aria-hidden="true" />
			시리즈 결과를 불러오는 중입니다.
		</div>
	</section>
{:else if !snapshot}
	<section
		class="grid min-h-56 place-items-center border border-rose-400/25 bg-rose-500/[0.035] px-4 text-center"
	>
		<div class="max-w-xl">
			<p class="text-sm font-semibold text-rose-100">시리즈 결과를 불러오지 못했습니다.</p>
			<p class="mt-2 break-words text-xs leading-5 text-rose-200">{loadError}</p>
			<button
				type="button"
				onclick={() => void loadResult()}
				class="mt-4 inline-flex h-8 items-center gap-1.5 border border-rose-300/30 px-3 text-xs font-semibold text-rose-100 hover:bg-rose-500/10"
			>
				<RefreshCw size={14} aria-hidden="true" /> 다시 시도
			</button>
		</div>
	</section>
{:else}
	<div class="space-y-3">
		{#if mutationError}
			<p
				class="border border-rose-400/25 bg-rose-500/[0.07] px-3 py-2 text-xs leading-5 font-semibold break-words text-rose-200"
				aria-live="assertive"
			>
				{mutationError}
			</p>
		{/if}
		{#if mutationNotice}
			<p
				class="border border-cyan-300/20 bg-cyan-400/[0.055] px-3 py-2 text-xs leading-5 font-semibold break-words text-cyan-100"
				aria-live="polite"
			>
				{mutationNotice}
			</p>
		{/if}
		{#if canRevertLastGame || canDelete}
			<div class="flex flex-wrap items-center justify-end gap-2">
				{#if canRevertLastGame}
					<button
						type="button"
						disabled={isMutating}
						onclick={openRevertLastGameDialog}
						class="inline-flex h-10 items-center gap-1.5 border border-cyan-300/25 bg-cyan-400/[0.06] px-3 text-xs font-semibold text-cyan-100 enabled:hover:bg-cyan-400/[0.12] disabled:cursor-not-allowed disabled:opacity-45"
					>
						{#if mutation === 'revertLastGame'}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
						{:else}
							<Undo2 size={14} aria-hidden="true" /> 마지막 결과 되돌리기
						{/if}
					</button>
				{/if}
				{#if canDelete}
					<button
						type="button"
						disabled={isMutating}
						onclick={openDeleteDialog}
						class="grid size-10 place-items-center border border-rose-300/25 text-rose-200 enabled:hover:bg-rose-400/10 disabled:cursor-not-allowed disabled:opacity-45"
						aria-label="시리즈 삭제"
						title="시리즈 삭제"
					>
						{#if mutation === 'delete'}
							<LoaderCircle size={15} class="animate-spin" aria-hidden="true" />
						{:else}
							<Trash2 size={15} aria-hidden="true" />
						{/if}
					</button>
				{/if}
			</div>
		{/if}
		<SeriesResultLedgerBoard catalog={data.championCatalog} {snapshot} />
	</div>
{/if}

<Dialog.Root bind:open={revertLastGameDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">마지막 결과 되돌리기</Dialog.Title
				>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="마지막 결과 되돌리기 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					마지막 확정 세트의 결과와 MMR 정산을 되돌린 뒤 픽밴 작업대로 이동합니다.
				</Dialog.Description>
				{#if revertLastGameError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{revertLastGameError}
					</p>
				{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={isMutating}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={isMutating}
					onclick={confirmRevertLastGame}
					class="inline-flex h-9 items-center gap-1.5 border border-cyan-300/35 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
				>
					{#if mutation === 'revertLastGame'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<Undo2 size={14} aria-hidden="true" /> 마지막 결과 되돌리기
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={deleteDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">시리즈 삭제</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="시리즈 삭제 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					확정된 모든 세트와 MMR 정산을 되돌린 뒤 시리즈를 취소 처리합니다. 계속하시겠습니까?
				</Dialog.Description>
				{#if deleteError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{deleteError}
					</p>
				{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={isMutating}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={isMutating}
					onclick={confirmDelete}
					class="inline-flex h-9 items-center gap-1.5 border border-rose-300/35 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 disabled:opacity-35"
				>
					{#if mutation === 'delete'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 삭제하는 중
					{:else}
						<Trash2 size={14} aria-hidden="true" /> 시리즈 삭제
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
