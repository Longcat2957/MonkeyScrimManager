<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { LoaderCircle, Play, RefreshCw, Trash2, X } from 'lucide-svelte';
	import { onMount } from 'svelte';
	import {
		createSeriesFromEntryDraft,
		entryDraftErrorText,
		fetchEntryDraft,
		isEntryDraftRevisionConflict,
		moveEntryDraftParticipant,
		reopenRecruitmentForEntry,
		resetEntryDraft
	} from '$lib/api/entryDraft';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import {
		cancelRecruitment,
		recruitmentManagementErrorText
	} from '$lib/api/recruitmentManagement';
	import EntryDraftBoard from '$lib/components/workflow/EntryDraftBoard.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import SeriesCompletedEntryRosterBoard from '$lib/components/workflow/SeriesCompletedEntryRosterBoard.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { EntryDraftSnapshot } from '$lib/contracts/entryDraft';
	import type { EntrySlot, MockEntryParticipant } from '$lib/fixtures/workflow';
	import {
		createBrowserRealtimeSocket,
		type BrowserRealtimeSocket,
		type RealtimeInvalidateEvent
	} from '$lib/realtime/browserSocket';
	import {
		entryDraftRealtimeTopics,
		isNewerEntryDraftInvalidation,
		type EntryDraftRealtimeTopics
	} from '$lib/realtime/entryDraft';
	import type { RealtimeConnectionState } from '$lib/realtime/types';
	import { formatRelativeTime } from '$lib/workflow/presentation';
	import type { PageProps } from './$types';

	type EntryView = 'draft' | 'preview';
	type EntryMutation = 'move' | 'reset' | 'createSeries' | 'reopen' | 'cancel';
	type SnapshotRefreshSource = 'manual' | 'realtime';
	const defaultRealtimeOfflineMessage = '변경 알림을 받을 수 없습니다.';
	type EntryRoute = {
		key: string;
		guildId: string;
		recruitmentId: string;
	};

	let { data }: PageProps = $props();
	let activeView = $state<EntryView>('draft');
	let snapshot = $state<EntryDraftSnapshot | null>(null);
	let loading = $state(true);
	let loadError = $state('');
	let mutation = $state<EntryMutation | null>(null);
	let mutationError = $state('');
	let mutationNotice = $state('');
	let recoveryRequired = $state(false);
	let recoveringAuthoritativeSnapshot = $state(false);
	let reopenDialogOpen = $state(false);
	let reopenError = $state('');
	let cancelDialogOpen = $state(false);
	let cancelError = $state('');
	let cancelIdempotencyKey = $state<string | null>(null);
	let activeRouteKey = $state<string | null>(null);
	let realtimeConnectionState = $state<RealtimeConnectionState>('reconnecting');
	let realtimeConflict = $state(false);
	let realtimeRefreshPending = $state(false);
	let realtimeOfflineMessage = $state(defaultRealtimeOfflineMessage);
	let entryRealtimeSocket: BrowserRealtimeSocket | null = null;
	const idempotencyKeys = createOperationIdempotencyKeys();

	const views: Array<{ value: EntryView; label: string }> = [
		{ value: 'draft', label: '편성' },
		{ value: 'preview', label: '미리보기' }
	];
	const participants = $derived(snapshot ? toBoardParticipants(snapshot) : []);
	const assignments = $derived(snapshot ? toBoardAssignments(snapshot) : {});
	const recruitment = $derived(snapshot?.recruitment ?? null);
	const isMutating = $derived(mutation !== null);
	const canEdit = $derived(Boolean(snapshot?.capabilities.canEdit));
	const canReset = $derived(Boolean(snapshot?.capabilities.canReset));
	const canCreateSeries = $derived(Boolean(snapshot?.capabilities.canCreateSeries));
	const canReopen = $derived(Boolean(snapshot?.capabilities.canReopen));
	const canCancel = $derived(Boolean(snapshot?.capabilities.canCancel));
	const hasCompleteAssignments = $derived(snapshot ? hasCompleteEntryAssignments(snapshot) : false);
	const canShowCreateSeries = $derived(canEdit || canCreateSeries);
	const mutationLocked = $derived(isMutating || recoveryRequired);
	const realtimeSaveState = $derived(
		realtimeConflict ? 'conflict' : isMutating ? 'saving' : 'saved'
	);
	const realtimeResourceLabel = $derived(
		recruitment ? `모집 #${recruitment.displayNumber} 엔트리 편성` : '엔트리 편성'
	);

	$effect(() => {
		const route = currentEntryRoute();
		if (route.key === activeRouteKey) return;

		activeRouteKey = route.key;
		resetRouteState();
		void loadEntryDraft(route);
	});

	$effect(() => {
		const route = currentEntryRoute();
		const topics = entryDraftRealtimeTopics(route.guildId, route.recruitmentId);
		let connectedOnce = false;
		const client = createBrowserRealtimeSocket({
			topics: topics.all,
			onConnectionStateChange: (state) => {
				if (!isRouteInView(route)) return;
				realtimeConnectionState = state;
				if (state !== 'connected') return;
				realtimeOfflineMessage = defaultRealtimeOfflineMessage;
				if (connectedOnce) queueRealtimeSnapshotRefresh(route);
				connectedOnce = true;
			},
			onInvalidate: (event) => handleRealtimeInvalidation(event, route, topics),
			onResyncRequired: () => queueRealtimeSnapshotRefresh(route),
			onProtocolError: (error) => {
				if (!isRouteInView(route)) return;
				realtimeOfflineMessage = error.message;
			}
		});

		entryRealtimeSocket = client;
		client.connect();
		return () => {
			if (entryRealtimeSocket === client) entryRealtimeSocket = null;
			client.disconnect();
		};
	});

	onMount(() => {
		const refreshWhenVisible = () => {
			if (globalThis.document.visibilityState !== 'visible') return;
			queueRealtimeSnapshotRefresh(currentEntryRoute());
		};
		globalThis.document.addEventListener('visibilitychange', refreshWhenVisible);
		return () => globalThis.document.removeEventListener('visibilitychange', refreshWhenVisible);
	});

	function toBoardParticipants(entrySnapshot: EntryDraftSnapshot): MockEntryParticipant[] {
		return entrySnapshot.draft.participants.map((participant) => ({
			id: participant.id,
			name: participant.displayName,
			roles: participant.preferredLanes,
			mainRole: participant.mainLane ?? participant.preferredLanes[0] ?? 'MID',
			mmr: participant.mmr,
			recentRecord:
				participant.games > 0
					? `내전 ${participant.wins}승 ${Math.max(participant.games - participant.wins, 0)}패`
					: '내전 전적 없음',
			riotTier: participant.riotTier,
			mainChampions: participant.championIds,
			profileIconId: participant.profileIconId
		}));
	}

	function toBoardAssignments(entrySnapshot: EntryDraftSnapshot): Record<string, EntrySlot> {
		return Object.fromEntries(
			entrySnapshot.draft.assignments.map((assignment) => [
				assignment.participantId,
				`${assignment.team}_${assignment.lane}` as EntrySlot
			])
		);
	}

	function hasCompleteEntryAssignments(entrySnapshot: EntryDraftSnapshot): boolean {
		const { participants, assignments } = entrySnapshot.draft;
		const targetCount = entrySnapshot.recruitment.targetCount;
		if (participants.length !== targetCount || assignments.length !== targetCount) return false;

		const participantIds = new Set(participants.map((participant) => participant.id));
		const assignedParticipantIds = new Set(
			assignments.map((assignment) => assignment.participantId)
		);
		const assignedSlots = new Set(
			assignments.map((assignment) => `${assignment.team}_${assignment.lane}`)
		);
		return (
			participantIds.size === targetCount &&
			assignedParticipantIds.size === targetCount &&
			assignedSlots.size === targetCount &&
			[...assignedParticipantIds].every((participantId) => participantIds.has(participantId))
		);
	}

	function entryDescription(): string {
		if (!recruitment) return '엔트리 정보를 불러오는 중';
		return `${recruitment.title} · 모집 #${recruitment.displayNumber} · ${participants.length}명`;
	}

	function currentEntryRoute(): EntryRoute {
		return {
			key: `${data.guildId}:${data.recruitmentId}`,
			guildId: data.guildId,
			recruitmentId: data.recruitmentId
		};
	}

	function isCurrentRoute(route: EntryRoute): boolean {
		return activeRouteKey === route.key;
	}

	function isRouteInView(route: EntryRoute): boolean {
		return currentEntryRoute().key === route.key;
	}

	function resetRouteState() {
		activeView = 'draft';
		snapshot = null;
		loading = true;
		loadError = '';
		mutation = null;
		mutationError = '';
		mutationNotice = '';
		recoveryRequired = false;
		recoveringAuthoritativeSnapshot = false;
		realtimeConnectionState = 'reconnecting';
		realtimeConflict = false;
		realtimeRefreshPending = false;
		realtimeOfflineMessage = defaultRealtimeOfflineMessage;
		reopenDialogOpen = false;
		reopenError = '';
		cancelDialogOpen = false;
		cancelError = '';
		cancelIdempotencyKey = null;
		idempotencyKeys.invalidate('entry-draft-move');
		idempotencyKeys.invalidate('entry-draft-reset');
		idempotencyKeys.invalidate('entry-draft-create-series');
		idempotencyKeys.invalidate('entry-draft-reopen');
	}

	async function loadEntryDraft(route = currentEntryRoute()) {
		if (!isCurrentRoute(route)) return;
		loading = true;
		loadError = '';
		try {
			const nextSnapshot = await fetchEntryDraft(route.guildId, route.recruitmentId);
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
			recoveryRequired = false;
			realtimeConflict = false;
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			loadError = entryDraftErrorText(cause);
		} finally {
			if (isCurrentRoute(route)) loading = false;
		}
	}

	async function recoverAuthoritativeSnapshot(
		conflictMessage: string | null,
		route = currentEntryRoute(),
		source: SnapshotRefreshSource = 'manual'
	) {
		if (!isCurrentRoute(route)) return;
		if (recoveringAuthoritativeSnapshot) return;

		recoveryRequired = true;
		recoveringAuthoritativeSnapshot = true;
		try {
			const nextSnapshot = await fetchEntryDraft(route.guildId, route.recruitmentId);
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
			recoveryRequired = false;
			realtimeConflict = false;
			if (source === 'manual') {
				mutationError = '';
				mutationNotice = conflictMessage
					? `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경된 배치를 확인한 뒤 다시 시도해 주세요.`
					: '서버 최신 상태를 불러왔습니다. 변경을 다시 시도할 수 있습니다.';
			}
		} catch (refreshCause) {
			if (!isCurrentRoute(route)) return;
			const refreshMessage = entryDraftErrorText(refreshCause);
			realtimeConflict = true;
			if (source === 'manual') {
				mutationError = conflictMessage
					? `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${refreshMessage}`
					: `최신 상태를 불러오지 못했습니다. ${refreshMessage}`;
			}
		} finally {
			if (isCurrentRoute(route)) {
				recoveringAuthoritativeSnapshot = false;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	function handleRealtimeInvalidation(
		event: RealtimeInvalidateEvent,
		route: EntryRoute,
		topics: EntryDraftRealtimeTopics
	) {
		if (!isRouteInView(route) || !snapshot) return;
		if (!isNewerEntryDraftInvalidation(event, snapshot, topics)) return;
		queueRealtimeSnapshotRefresh(route);
	}

	function queueRealtimeSnapshotRefresh(route: EntryRoute) {
		if (!isCurrentRoute(route) || !snapshot) return;
		realtimeRefreshPending = true;
		void flushRealtimeSnapshotRefresh(route);
	}

	async function flushRealtimeSnapshotRefresh(route: EntryRoute) {
		if (!isCurrentRoute(route) || !realtimeRefreshPending) return;
		if (isMutating || recoveringAuthoritativeSnapshot) return;

		realtimeRefreshPending = false;
		await recoverAuthoritativeSnapshot(null, route, 'realtime');
	}

	function reconnectRealtime() {
		realtimeOfflineMessage = defaultRealtimeOfflineMessage;
		entryRealtimeSocket?.reconnect();
	}

	async function recoverAfterRevisionConflict(
		cause: unknown,
		operation: EntryMutation,
		route: EntryRoute
	) {
		idempotencyKeys.invalidate(`entry-draft-${operation}`);
		realtimeConflict = true;
		await recoverAuthoritativeSnapshot(entryDraftErrorText(cause), route);
	}

	async function retryAuthoritativeSnapshot() {
		mutationError = '';
		mutationNotice = '';
		await recoverAuthoritativeSnapshot(null);
	}

	async function moveParticipant(participantId: string, slot: EntrySlot | null) {
		const route = currentEntryRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canEdit || mutationLocked) return;

		const operation = { participantId, slot, expectedRevision: currentSnapshot.draft.revision };
		const idempotencyKey = idempotencyKeys.keyFor('entry-draft-move', operation);
		mutation = 'move';
		mutationError = '';
		mutationNotice = '';
		try {
			const nextSnapshot = await moveEntryDraftParticipant(route.guildId, route.recruitmentId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
			idempotencyKeys.complete('entry-draft-move');
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isEntryDraftRevisionConflict(cause)) {
				await recoverAfterRevisionConflict(cause, 'move', route);
			} else {
				mutationError = entryDraftErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function resetDraft() {
		const route = currentEntryRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canReset || mutationLocked) return;

		const operation = { expectedRevision: currentSnapshot.draft.revision };
		const idempotencyKey = idempotencyKeys.keyFor('entry-draft-reset', operation);
		mutation = 'reset';
		mutationError = '';
		mutationNotice = '';
		try {
			const nextSnapshot = await resetEntryDraft(route.guildId, route.recruitmentId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
			idempotencyKeys.complete('entry-draft-reset');
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isEntryDraftRevisionConflict(cause)) {
				await recoverAfterRevisionConflict(cause, 'reset', route);
			} else {
				mutationError = entryDraftErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function createSeries() {
		const route = currentEntryRoute();
		const currentSnapshot = snapshot;
		if (
			!isCurrentRoute(route) ||
			!currentSnapshot ||
			!canCreateSeries ||
			!hasCompleteAssignments ||
			mutationLocked
		) {
			return;
		}

		const operation = {
			sourceRecruitmentId: currentSnapshot.recruitment.id,
			expectedRevision: currentSnapshot.recruitment.revision
		};
		const idempotencyKey = idempotencyKeys.keyFor('entry-draft-create-series', operation);
		mutation = 'createSeries';
		mutationError = '';
		mutationNotice = '';
		try {
			const series = await createSeriesFromEntryDraft(route.guildId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('entry-draft-create-series');
			await goto(series.href);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isEntryDraftRevisionConflict(cause)) {
				await recoverAfterRevisionConflict(cause, 'createSeries', route);
			} else {
				mutationError = entryDraftErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	function openReopenDialog() {
		if (!snapshot || !canReopen || mutationLocked) return;
		reopenError = '';
		reopenDialogOpen = true;
	}

	async function reopenRecruitment() {
		const route = currentEntryRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canReopen || mutationLocked) return;

		const operation = { expectedRevision: currentSnapshot.recruitment.revision };
		const idempotencyKey = idempotencyKeys.keyFor('entry-draft-reopen', operation);
		mutation = 'reopen';
		mutationError = '';
		mutationNotice = '';
		reopenError = '';
		try {
			const nextSnapshot = await reopenRecruitmentForEntry(route.guildId, route.recruitmentId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			snapshot = nextSnapshot;
			idempotencyKeys.complete('entry-draft-reopen');
			await goto(
				resolve('/g/[guildId]/recruitments/[id]', {
					guildId: route.guildId,
					id: route.recruitmentId
				})
			);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isEntryDraftRevisionConflict(cause)) {
				reopenDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'reopen', route);
			} else {
				reopenError = entryDraftErrorText(cause);
				mutationError = reopenError;
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	function openCancelDialog() {
		if (!snapshot || !canCancel || mutationLocked) return;
		cancelError = '';
		cancelDialogOpen = true;
	}

	async function confirmCancellation() {
		const route = currentEntryRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canCancel || mutationLocked) return;

		const idempotencyKey = (cancelIdempotencyKey ??= globalThis.crypto.randomUUID());
		mutation = 'cancel';
		cancelError = '';
		mutationError = '';
		mutationNotice = '';
		try {
			await cancelRecruitment(route.guildId, currentSnapshot.recruitment.id, idempotencyKey);
			if (!isCurrentRoute(route)) return;
			await goto(resolve('/g/[guildId]/series', { guildId: route.guildId }));
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			cancelError = recruitmentManagementErrorText(cause);
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}
</script>

<svelte:head>
	<title>엔트리 편성 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader kicker="일반 내전" title="엔트리 편성" description={entryDescription()}>
		{#snippet actions()}
			<div class="flex flex-wrap items-center justify-end gap-2">
				<div
					class="flex border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
					role="tablist"
					aria-label="엔트리 화면"
				>
					{#each views as view (view.value)}
						<button
							type="button"
							role="tab"
							aria-selected={activeView === view.value}
							disabled={!snapshot || mutationLocked}
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
				{#if canShowCreateSeries}
					<button
						type="button"
						disabled={mutationLocked || !canCreateSeries || !hasCompleteAssignments}
						onclick={createSeries}
						class="inline-flex h-10 items-center gap-1.5 border border-cyan-300/35 bg-cyan-400/15 px-3 text-xs font-semibold text-cyan-50 enabled:hover:bg-cyan-400/25 disabled:cursor-not-allowed disabled:opacity-45"
					>
						{#if mutation === 'createSeries'}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 시리즈 시작 중
						{:else}
							<Play size={14} aria-hidden="true" /> 시리즈 시작
						{/if}
					</button>
				{/if}
				{#if canReopen}
					<button
						type="button"
						disabled={mutationLocked}
						onclick={openReopenDialog}
						class="inline-flex h-10 items-center gap-1.5 border border-amber-300/30 bg-amber-400/[0.06] px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/[0.12] disabled:cursor-not-allowed disabled:opacity-45"
					>
						{#if mutation === 'reopen'}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
						{/if}
						모집으로 되돌리기
					</button>
				{/if}
				{#if canCancel}
					<button
						type="button"
						disabled={mutationLocked}
						onclick={openCancelDialog}
						class="grid size-10 place-items-center border border-rose-300/25 text-rose-200 enabled:hover:bg-rose-400/10 disabled:cursor-not-allowed disabled:opacity-45"
						aria-label="모집 삭제"
						title="모집 삭제"
					>
						<Trash2 size={15} aria-hidden="true" />
					</button>
				{/if}
			</div>
		{/snippet}
	</WorkspaceHeader>

	{#if snapshot}
		<RealtimeEditStateBar
			resourceLabel={realtimeResourceLabel}
			connectionState={realtimeConnectionState}
			saveState={realtimeSaveState}
			showEditors={false}
			lastSavedAt={formatRelativeTime(snapshot.draft.updatedAt)}
			offlineMessage={realtimeOfflineMessage}
			conflictMessage={mutationError ||
				'다른 운영자의 변경을 확인해야 현재 배치를 다시 수정할 수 있습니다.'}
			onReconnect={reconnectRealtime}
			onAcceptServer={() => void retryAuthoritativeSnapshot()}
		/>
	{/if}

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
			class="border border-amber-300/25 bg-amber-400/[0.06] px-3 py-2 text-xs leading-5 font-semibold break-words text-amber-100"
			aria-live="polite"
		>
			{mutationNotice}
		</p>
	{/if}
	{#if recoveryRequired}
		<div
			class="flex flex-wrap items-center justify-between gap-3 border border-amber-300/25 bg-amber-400/[0.04] px-3 py-2"
		>
			<p class="text-xs leading-5 text-amber-100">
				최신 엔트리 상태를 확인해야 다시 변경할 수 있습니다.
			</p>
			<button
				type="button"
				disabled={recoveringAuthoritativeSnapshot}
				onclick={retryAuthoritativeSnapshot}
				class="inline-flex h-8 items-center gap-1.5 border border-amber-300/30 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/10 disabled:cursor-not-allowed disabled:opacity-45"
			>
				{#if recoveringAuthoritativeSnapshot}
					<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 최신 상태 불러오는 중
				{:else}
					<RefreshCw size={14} aria-hidden="true" /> 최신 상태 다시 불러오기
				{/if}
			</button>
		</div>
	{/if}

	{#if loading && !snapshot}
		<section
			class="flex min-h-56 items-center justify-center border border-[var(--color-app-border)] bg-[var(--color-app-surface)] px-4 text-center"
			aria-busy="true"
		>
			<div class="grid justify-items-center gap-2 text-sm font-semibold text-slate-400">
				<LoaderCircle size={20} class="animate-spin text-cyan-200" aria-hidden="true" />
				엔트리 정보를 불러오는 중입니다.
			</div>
		</section>
	{:else if !snapshot}
		<section
			class="grid min-h-56 place-items-center border border-rose-400/25 bg-rose-500/[0.035] px-4 text-center"
		>
			<div class="max-w-xl">
				<p class="text-sm font-semibold text-rose-100">엔트리 정보를 불러오지 못했습니다.</p>
				<p class="mt-2 break-words text-xs leading-5 text-rose-200">{loadError}</p>
				<button
					type="button"
					onclick={() => void loadEntryDraft()}
					class="mt-4 inline-flex h-8 items-center gap-1.5 border border-rose-300/30 px-3 text-xs font-semibold text-rose-100 hover:bg-rose-500/10"
				>
					<RefreshCw size={14} aria-hidden="true" /> 다시 시도
				</button>
			</div>
		</section>
	{:else if activeView === 'draft'}
		<EntryDraftBoard
			{participants}
			{assignments}
			targetCount={snapshot.recruitment.targetCount}
			profileIconVersion={data.championCatalog.version}
			readOnly={!canEdit}
			busy={mutationLocked}
			{canReset}
			onMoveParticipant={moveParticipant}
			onReset={resetDraft}
		/>
	{:else}
		<SeriesCompletedEntryRosterBoard
			{participants}
			{assignments}
			champions={data.championCatalog.champions}
			title={`${snapshot.recruitment.title} 엔트리`}
			subtitle="현재 배치를 확인합니다"
			matchFormat="일반 내전"
			profileIconVersion={data.championCatalog.version}
		/>
	{/if}
</div>

<Dialog.Root bind:open={reopenDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">모집으로 되돌리기</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="모집으로 되돌리기 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					모집 #{snapshot?.recruitment.displayNumber}을 다시 열면 엔트리 편성 단계를 종료하고
					모집 상태로 돌아갑니다. 현재 참가자와 엔트리 배치는 유지됩니다.
				</Dialog.Description>
				{#if reopenError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{reopenError}
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
					onclick={reopenRecruitment}
					class="inline-flex h-9 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 disabled:opacity-35"
				>
					{#if mutation === 'reopen'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						모집으로 되돌리기
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={cancelDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">모집 삭제</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="모집 삭제 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					모집 #{snapshot?.recruitment.displayNumber}을 삭제하면 현재 엔트리 배치를 더 이상 진행할
					수 없습니다.
				</Dialog.Description>
				{#if cancelError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{cancelError}
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
					onclick={confirmCancellation}
					class="inline-flex h-9 items-center gap-1.5 border border-rose-300/35 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 disabled:opacity-35"
				>
					{#if mutation === 'cancel'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 삭제 중
					{:else}
						<Trash2 size={14} aria-hidden="true" /> 모집 삭제
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
