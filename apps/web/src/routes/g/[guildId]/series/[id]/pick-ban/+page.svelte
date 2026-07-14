<script lang="ts">
	import { goto } from '$app/navigation';
	import { Dialog } from 'bits-ui';
	import { LoaderCircle, RefreshCw, RotateCcw, Trash2, X } from 'lucide-svelte';
	import {
		deleteSeries,
		fetchSeriesPickBan,
		revertSeries,
		seriesPickBanErrorText
	} from '$lib/api/seriesPickBan';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import ScrimPickBanResultInputBoard from '$lib/components/workflow/ScrimPickBanResultInputBoard.svelte';
	import ScrimPickBanResultSetSummaryCard from '$lib/components/workflow/ScrimPickBanResultSetSummaryCard.svelte';
	import type { SeriesPickBanSnapshot } from '$lib/contracts/seriesPickBan';
	import type { PageProps } from './$types';

	type PickBanView = 'input' | 'summary';
	type SeriesMutation = 'revert' | 'delete';
	type PickBanRoute = {
		key: string;
		guildId: string;
		seriesId: string;
	};

	let { data }: PageProps = $props();
	let activeView = $state<PickBanView>('input');
	let snapshot = $state<SeriesPickBanSnapshot | null>(null);
	let loading = $state(true);
	let loadError = $state('');
	let mutation = $state<SeriesMutation | null>(null);
	let mutationError = $state('');
	let revertDialogOpen = $state(false);
	let revertError = $state('');
	let deleteDialogOpen = $state(false);
	let deleteError = $state('');
	let activeRouteKey = $state<string | null>(null);
	const idempotencyKeys = createOperationIdempotencyKeys();

	const views: Array<{ value: PickBanView; label: string }> = [
		{ value: 'input', label: '결과 입력' },
		{ value: 'summary', label: '세트 요약' }
	];
	const isMutating = $derived(mutation !== null);
	const canRevert = $derived(Boolean(snapshot?.capabilities.canRevert));
	const canDelete = $derived(Boolean(snapshot?.capabilities.canDelete));
	const seriesDescription = $derived(
		snapshot
			? `${snapshot.series.title} · BO${snapshot.series.bestOf} · ${snapshot.currentGame.setNumber}세트`
			: `시리즈 #${data.seriesId} 정보를 불러오는 중`
	);

	$effect(() => {
		const route = currentPickBanRoute();
		if (route.key === activeRouteKey) return;

		activeRouteKey = route.key;
		resetRouteState();
		void loadPickBan(route);
	});

	function currentPickBanRoute(): PickBanRoute {
		return {
			key: `${data.guildId}:${data.seriesId}`,
			guildId: data.guildId,
			seriesId: data.seriesId
		};
	}

	function isCurrentRoute(route: PickBanRoute): boolean {
		return activeRouteKey === route.key;
	}

	function resetRouteState() {
		activeView = 'input';
		snapshot = null;
		loading = true;
		loadError = '';
		mutation = null;
		mutationError = '';
		revertDialogOpen = false;
		revertError = '';
		deleteDialogOpen = false;
		deleteError = '';
		idempotencyKeys.invalidate('series-revert');
		idempotencyKeys.invalidate('series-delete');
	}

	async function loadPickBan(route = currentPickBanRoute()) {
		if (!isCurrentRoute(route)) return;
		loading = true;
		loadError = '';
		try {
			const nextSnapshot = await fetchSeriesPickBan(route.guildId, route.seriesId);
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			loadError = seriesPickBanErrorText(cause);
		} finally {
			if (isCurrentRoute(route)) loading = false;
		}
	}

	function openRevertDialog() {
		if (!snapshot || !canRevert || isMutating) return;
		deleteDialogOpen = false;
		revertError = '';
		mutationError = '';
		revertDialogOpen = true;
	}

	function openDeleteDialog() {
		if (!snapshot || !canDelete || isMutating) return;
		revertDialogOpen = false;
		deleteError = '';
		mutationError = '';
		deleteDialogOpen = true;
	}

	async function confirmRevert() {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canRevert || isMutating) return;

		const operation = { expectedRevision: currentSnapshot.series.revision };
		const idempotencyKey = idempotencyKeys.keyFor('series-revert', operation);
		mutation = 'revert';
		mutationError = '';
		revertError = '';
		try {
			const result = await revertSeries(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-revert');
			await goto(result.nextHref);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			revertError = seriesPickBanErrorText(cause);
			mutationError = revertError;
		} finally {
			if (isCurrentRoute(route)) mutation = null;
		}
	}

	async function confirmDelete() {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canDelete || isMutating) return;

		const operation = { expectedRevision: currentSnapshot.series.revision };
		const idempotencyKey = idempotencyKeys.keyFor('series-delete', operation);
		mutation = 'delete';
		mutationError = '';
		deleteError = '';
		try {
			const result = await deleteSeries(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-delete');
			await goto(result.nextHref);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			deleteError = seriesPickBanErrorText(cause);
			mutationError = deleteError;
		} finally {
			if (isCurrentRoute(route)) mutation = null;
		}
	}

	function teamFor(
		snapshot: SeriesPickBanSnapshot,
		teamId: 'TEAM_1' | 'TEAM_2',
		side: 'BLUE' | 'RED'
	) {
		const team = snapshot.teams.find((candidate) => candidate.team === teamId);
		if (!team) throw new Error(`Missing ${teamId} in a validated pick-ban snapshot`);
		return {
			side,
			name: team.name,
			players: team.players.map((player) => ({
				id: player.id,
				name: player.displayName,
				lane: player.lane,
				mmr: player.mmr,
				riotTier: player.riotTier,
				preferredChampions: [...player.preferredChampionIds],
				frequentChampions: []
			}))
		};
	}

	function boardTeamsFor(snapshot: SeriesPickBanSnapshot) {
		const teamOneSide = snapshot.currentGame.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
		return {
			BLUE:
				teamOneSide === 'BLUE'
					? teamFor(snapshot, 'TEAM_1', 'BLUE')
					: teamFor(snapshot, 'TEAM_2', 'BLUE'),
			RED:
				teamOneSide === 'RED'
					? teamFor(snapshot, 'TEAM_1', 'RED')
					: teamFor(snapshot, 'TEAM_2', 'RED')
		};
	}

	function boardGameFor(snapshot: SeriesPickBanSnapshot) {
		return {
			id: `${snapshot.series.id}-set-${snapshot.currentGame.setNumber}`,
			label: `${snapshot.currentGame.setNumber}세트`,
			sidePreset: snapshot.currentGame.sidePreset,
			winner: snapshot.currentGame.winner,
			bans: {
				BLUE: [...snapshot.currentGame.bans.BLUE],
				RED: [...snapshot.currentGame.bans.RED]
			},
			picks: { ...snapshot.currentGame.picks }
		};
	}
</script>

<svelte:head>
	<title>픽밴 작업대 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader kicker="일반 내전" title="픽밴 작업대" description={seriesDescription}>
		{#snippet actions()}
			<div class="flex flex-wrap items-center justify-end gap-2">
				<div
					class="flex border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
					role="tablist"
					aria-label="픽밴 화면"
				>
					{#each views as view (view.value)}
						<button
							type="button"
							role="tab"
							aria-selected={activeView === view.value}
							disabled={!snapshot || isMutating}
							onclick={() => (activeView = view.value)}
							class={`h-8 px-3 text-xs font-semibold disabled:cursor-not-allowed disabled:opacity-35 ${
								activeView === view.value
									? 'rounded-sm bg-cyan-400/12 text-cyan-100'
									: 'text-slate-500 enabled:hover:text-slate-200'
							}`}
						>
							{view.label}
						</button>
					{/each}
				</div>
				{#if canRevert}
					<button
						type="button"
						disabled={isMutating}
						onclick={openRevertDialog}
						class="inline-flex h-10 items-center gap-1.5 border border-amber-300/30 bg-amber-400/[0.06] px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/[0.12] disabled:cursor-not-allowed disabled:opacity-45"
					>
						{#if mutation === 'revert'}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
						{:else}
							<RotateCcw size={14} aria-hidden="true" /> 엔트리로 되돌리기
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
		{/snippet}
	</WorkspaceHeader>

	{#if mutationError}
		<p
			class="border border-rose-400/25 bg-rose-500/[0.07] px-3 py-2 text-xs leading-5 font-semibold break-words text-rose-200"
			aria-live="assertive"
		>
			{mutationError}
		</p>
	{/if}

	{#if loading && !snapshot}
		<section
			class="flex min-h-56 items-center justify-center border border-[var(--color-app-border)] bg-[var(--color-app-surface)] px-4 text-center"
			aria-busy="true"
		>
			<div class="grid justify-items-center gap-2 text-sm font-semibold text-slate-400">
				<LoaderCircle size={20} class="animate-spin text-cyan-200" aria-hidden="true" />
				시리즈 정보를 불러오는 중입니다.
			</div>
		</section>
	{:else if !snapshot}
		<section
			class="grid min-h-56 place-items-center border border-rose-400/25 bg-rose-500/[0.035] px-4 text-center"
		>
			<div class="max-w-xl">
				<p class="text-sm font-semibold text-rose-100">시리즈 정보를 불러오지 못했습니다.</p>
				<p class="mt-2 break-words text-xs leading-5 text-rose-200">{loadError}</p>
				<button
					type="button"
					onclick={() => void loadPickBan()}
					class="mt-4 inline-flex h-8 items-center gap-1.5 border border-rose-300/30 px-3 text-xs font-semibold text-rose-100 hover:bg-rose-500/10"
				>
					<RefreshCw size={14} aria-hidden="true" /> 다시 시도
				</button>
			</div>
		</section>
	{:else}
		{@const boardTeams = boardTeamsFor(snapshot)}
		{@const boardGame = boardGameFor(snapshot)}
		{#if activeView === 'input'}
			{#key `${snapshot.series.id}:${snapshot.currentGame.setNumber}:${snapshot.series.revision}`}
				<ScrimPickBanResultInputBoard
					teams={boardTeams}
					teamsAreGameSides
					initialGame={boardGame}
					champions={data.championCatalog.champions}
					patchVersion={snapshot.currentGame.patchVersion ?? data.championCatalog.version}
				/>
			{/key}
		{:else}
			<ScrimPickBanResultSetSummaryCard
				label={boardGame.label}
				blueTeam={boardTeams.BLUE}
				redTeam={boardTeams.RED}
				winner={boardGame.winner}
				bans={boardGame.bans}
				picks={boardGame.picks}
				champions={data.championCatalog.champions}
				version={snapshot.currentGame.patchVersion ?? data.championCatalog.version}
			/>
		{/if}
	{/if}
</div>

<Dialog.Root bind:open={revertDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50"
					>엔트리 편성으로 되돌리기</Dialog.Title
				>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="엔트리 편성으로 되돌리기 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					확정된 게임이 없는 시리즈만 엔트리 편성으로 되돌릴 수 있습니다. 게임 결과가 있으면 마지막
					결과부터 되돌린 뒤 다시 시도해 주세요.
				</Dialog.Description>
				{#if revertError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{revertError}
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
					onclick={confirmRevert}
					class="inline-flex h-9 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 disabled:opacity-35"
				>
					{#if mutation === 'revert'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<RotateCcw size={14} aria-hidden="true" /> 시리즈 되돌리기
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
					확정된 게임이 없는 시리즈만 삭제할 수 있습니다. 게임 결과가 있으면 마지막 결과부터 되돌린
					뒤 다시 시도해 주세요.
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
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 삭제 중
					{:else}
						<Trash2 size={14} aria-hidden="true" /> 시리즈 삭제
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
