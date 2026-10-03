<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, LoaderCircle } from 'lucide-svelte';
	import { onMount } from 'svelte';
	import {
		auctionTournamentDraftErrorText,
		executeAuctionTournamentDraftCommand,
		fetchAuctionTournamentDraft,
		isAuctionTournamentDraftRevisionConflict
	} from '$lib/api/auctionTournamentDraft';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentBidDraftWorkbench from '$lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte';
	import type {
		AuctionTournamentDraftCommand,
		AuctionTournamentDraftSnapshot
	} from '$lib/contracts/auctionTournamentDraft';
	import {
		createBrowserRealtimeSocket,
		type BrowserRealtimeSocket,
		type RealtimeInvalidateEvent
	} from '$lib/realtime/browserSocket';
	import {
		auctionTournamentDraftRealtimeTopics,
		isNewerAuctionTournamentDraftInvalidation,
		type AuctionTournamentDraftRealtimeTopics
	} from '$lib/realtime/auctionTournamentDraft';
	import type { RealtimeConnectionState } from '$lib/realtime/types';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import AuctionTournamentCancelControl from '$lib/components/workflow/AuctionTournamentCancelControl.svelte';
	import AuctionTournamentFinishControl from '$lib/components/workflow/AuctionTournamentFinishControl.svelte';
	import type { PageProps } from './$types';

	type AuctionDraftRoute = {
		key: string;
		guildId: string;
		tournamentId: string;
	};

	const defaultRealtimeOfflineMessage = '입찰 변경 알림을 받을 수 없습니다.';

	let { data }: PageProps = $props();

	let snapshotOverride = $state<AuctionTournamentDraftSnapshot | null>(null);
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let submitting = $state(false);
	let mutationError = $state('');
	let realtimeConnectionState = $state<RealtimeConnectionState>('reconnecting');
	let realtimeConflict = $state(false);
	let realtimeRefreshPending = $state(false);
	let realtimeRefreshing = $state(false);
	let realtimeOfflineMessage = $state(defaultRealtimeOfflineMessage);
	let auctionDraftRealtimeSocket: BrowserRealtimeSocket | null = null;
	let idempotencyKeys = createOperationIdempotencyKeys();

	const incomingSnapshotKey = $derived(
		`${data.snapshot.tournament.id}:${data.snapshot.tournament.revision}`
	);
	const snapshot = $derived(
		snapshotOverride !== null && snapshotOverrideSourceKey === incomingSnapshotKey
			? snapshotOverride
			: data.snapshot
	);
	const tournament = $derived(snapshot.tournament);
	const formatLabel = $derived(tournament.format === 'TEN_PERSON' ? '10인 경매' : '20인 경매');
	const rulesLabel = $derived(formatAuctionRules(tournament.format, tournament.rules));
	const realtimeSaveState = $derived(
		realtimeConflict ? 'conflict' : submitting ? 'saving' : 'saved'
	);
	const realtimeResourceLabel = $derived(`토너먼트 #${tournament.displayNumber} 입찰`);
	const shouldUseRealtime = $derived(
		snapshot.permissions.canManageDraft && tournament.status === 'BIDDING'
	);

	$effect(() => {
		const route = currentAuctionDraftRoute();
		if (!shouldUseRealtime) return;

		const topics = auctionTournamentDraftRealtimeTopics(route.guildId, route.tournamentId);
		let connectedOnce = false;
		const client = createBrowserRealtimeSocket({
			topics: topics.all,
			onConnectionStateChange: (state) => {
				if (!isCurrentAuctionDraftRoute(route)) return;
				realtimeConnectionState = state;
				if (state !== 'connected') return;
				realtimeOfflineMessage = defaultRealtimeOfflineMessage;
				if (connectedOnce) queueRealtimeSnapshotRefresh(route);
				connectedOnce = true;
			},
			onInvalidate: (event) => handleRealtimeInvalidation(event, route, topics),
			onResyncRequired: () => queueRealtimeSnapshotRefresh(route),
			onProtocolError: (error) => {
				if (!isCurrentAuctionDraftRoute(route)) return;
				realtimeOfflineMessage = error.message;
			}
		});

		auctionDraftRealtimeSocket = client;
		client.connect();
		return () => {
			if (auctionDraftRealtimeSocket === client) auctionDraftRealtimeSocket = null;
			client.disconnect();
		};
	});

	onMount(() => {
		const refreshWhenVisible = () => {
			if (globalThis.document.visibilityState !== 'visible') return;
			queueRealtimeSnapshotRefresh(currentAuctionDraftRoute());
		};
		globalThis.document.addEventListener('visibilitychange', refreshWhenVisible);
		return () => globalThis.document.removeEventListener('visibilitychange', refreshWhenVisible);
	});

	function applySnapshot(nextSnapshot: AuctionTournamentDraftSnapshot) {
		snapshotOverrideSourceKey = incomingSnapshotKey;
		snapshotOverride = nextSnapshot;
	}

	function currentAuctionDraftRoute(): AuctionDraftRoute {
		const tournamentId = data.snapshot.tournament.id;
		return {
			key: `${data.guildId}:${tournamentId}`,
			guildId: data.guildId,
			tournamentId
		};
	}

	function isCurrentAuctionDraftRoute(route: AuctionDraftRoute): boolean {
		return currentAuctionDraftRoute().key === route.key;
	}

	function handleRealtimeInvalidation(
		event: RealtimeInvalidateEvent,
		route: AuctionDraftRoute,
		topics: AuctionTournamentDraftRealtimeTopics
	) {
		if (!isCurrentAuctionDraftRoute(route)) return;
		if (!isNewerAuctionTournamentDraftInvalidation(event, snapshot, topics)) return;
		queueRealtimeSnapshotRefresh(route);
	}

	function queueRealtimeSnapshotRefresh(route: AuctionDraftRoute) {
		if (!isCurrentAuctionDraftRoute(route)) return;
		realtimeRefreshPending = true;
		void flushRealtimeSnapshotRefresh(route);
	}

	async function flushRealtimeSnapshotRefresh(route: AuctionDraftRoute) {
		if (!isCurrentAuctionDraftRoute(route) || !realtimeRefreshPending) return;
		if (submitting || realtimeRefreshing) return;

		realtimeRefreshPending = false;
		realtimeRefreshing = true;
		try {
			const nextSnapshot = await fetchAuctionTournamentDraft(route.guildId, route.tournamentId);
			if (!isCurrentAuctionDraftRoute(route)) return;
			applySnapshot(nextSnapshot);
			realtimeConflict = false;
		} catch (cause) {
			if (!isCurrentAuctionDraftRoute(route)) return;
			realtimeConflict = true;
			mutationError = `입찰 최신 상태를 불러오지 못했습니다. ${auctionTournamentDraftErrorText(cause)}`;
		} finally {
			if (isCurrentAuctionDraftRoute(route)) {
				realtimeRefreshing = false;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	function reconnectRealtime() {
		realtimeOfflineMessage = defaultRealtimeOfflineMessage;
		auctionDraftRealtimeSocket?.reconnect();
	}

	function refreshAuthoritativeSnapshot() {
		mutationError = '';
		realtimeConflict = false;
		queueRealtimeSnapshotRefresh(currentAuctionDraftRoute());
	}

	async function recoverAuthoritativeSnapshot(conflictMessage: string) {
		try {
			const nextSnapshot = await fetchAuctionTournamentDraft(data.guildId, tournament.id);
			applySnapshot(nextSnapshot);
			realtimeConflict = false;
			mutationError = `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
		} catch (refreshCause) {
			realtimeConflict = true;
			mutationError = `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${auctionTournamentDraftErrorText(refreshCause)}`;
		}
	}

	async function handleDraftCommand(command: AuctionTournamentDraftCommand): Promise<void> {
		const allowed =
			command.kind === 'REVERT_LAST'
				? snapshot.permissions.canRevertLast
				: command.kind === 'RESET_TO_INITIAL'
					? snapshot.permissions.canManageDraft || snapshot.permissions.canRevertLast
					: snapshot.permissions.canManageDraft;
		if (submitting || !allowed) return;
		if (command.expectedRevision !== snapshot.tournament.revision) {
			await recoverAuthoritativeSnapshot('경매 드래프트 버전이 변경되었습니다.');
			throw new Error('stale_auction_tournament_draft_command');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-tournament-draft-command', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		submitting = true;
		mutationError = '';
		try {
			const nextSnapshot = await executeAuctionTournamentDraftCommand(
				data.guildId,
				tournament.id,
				command,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			realtimeConflict = false;
			idempotencyKeys.complete('auction-tournament-draft-command');
		} catch (cause) {
			if (isAuctionTournamentDraftRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-draft-command');
				await recoverAuthoritativeSnapshot(auctionTournamentDraftErrorText(cause));
			} else {
				mutationError = auctionTournamentDraftErrorText(cause);
			}
			throw cause;
		} finally {
			submitting = false;
			void flushRealtimeSnapshotRefresh(currentAuctionDraftRoute());
		}
	}
</script>

<svelte:head>
	<title>경매 드래프트 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="경매 드래프트"
		description={`${tournament.title} · 토너먼트 #${tournament.displayNumber} · ${formatLabel} · ${rulesLabel}`}
	>
		{#snippet actions()}
			<AuctionTournamentFinishControl
				guildId={data.guildId}
				tournamentId={tournament.id}
				canFinish={snapshot.permissions.canCancel &&
					tournament.status !== 'COMPLETED' &&
					tournament.status !== 'CANCELLED'}
				busy={submitting}
			/>
			<AuctionTournamentCancelControl
				guildId={data.guildId}
				{tournament}
				canCancel={snapshot.permissions.canCancel}
				busy={submitting}
			/>
			{#if tournament.status !== 'BIDDING'}
				<a
					href={resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
						guildId: data.guildId,
						id: tournament.id
					})}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
				>
					브래킷 <ArrowRight size={14} aria-hidden="true" />
				</a>
			{/if}
		{/snippet}
	</WorkspaceHeader>

	{#if shouldUseRealtime}
		<RealtimeEditStateBar
			resourceLabel={realtimeResourceLabel}
			connectionState={realtimeConnectionState}
			saveState={realtimeSaveState}
			showEditors={false}
			conflictMessage="다른 운영자의 입찰 변경을 최신 상태로 반영하지 못했습니다."
			offlineMessage={realtimeOfflineMessage}
			onReconnect={reconnectRealtime}
			onAcceptServer={() => void refreshAuthoritativeSnapshot()}
		/>
	{/if}

	{#if mutationError}
		<div
			role="alert"
			class="flex min-h-10 items-center gap-2 border border-rose-300/30 bg-rose-400/10 px-3 text-xs font-medium text-rose-100"
		>
			{mutationError}
		</div>
	{/if}

	{#if submitting}
		<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
			<LoaderCircle size={15} class="animate-spin" aria-hidden="true" /> 경매 드래프트 반영 중
		</div>
	{/if}

	<AuctionTournamentBidDraftWorkbench
		{snapshot}
		onCommand={handleDraftCommand}
		{submitting}
		championCatalog={data.championCatalog}
	/>
</div>
