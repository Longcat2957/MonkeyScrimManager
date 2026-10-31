<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { Flag, LoaderCircle, RefreshCw, RotateCcw, Trash2, Undo2, X } from 'lucide-svelte';
	import { onMount } from 'svelte';
	import {
		completeSeriesEarly,
		deleteSeries,
		fetchSeriesPickBan,
		isSeriesPickBanRevisionConflict,
		revertLastSeriesPickBanGame,
		SeriesPickBanApiError,
		revertSeries,
		seriesPickBanErrorText,
		submitSeriesPickBanGame,
		updateSeriesPickBanDraft
	} from '$lib/api/seriesPickBan';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import { fetchSeriesResult } from '$lib/api/seriesResult';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import ScrimPickBanConfirmedSetsDisclosure from '$lib/components/workflow/ScrimPickBanConfirmedSetsDisclosure.svelte';
	import ScrimPickBanResultInputBoard from '$lib/components/workflow/ScrimPickBanResultInputBoard.svelte';
	import type {
		SeriesEarlyCompletionOutcome,
		SeriesPickBanDraftOperation,
		SeriesPickBanSnapshot
	} from '$lib/contracts/seriesPickBan';
	import {
		createBrowserRealtimeSocket,
		type BrowserRealtimeSocket,
		type RealtimeInvalidateEvent
	} from '$lib/realtime/browserSocket';
	import {
		isNewerSeriesPickBanInvalidation,
		seriesPickBanRealtimeTopics,
		type SeriesPickBanRealtimeTopics
	} from '$lib/realtime/seriesPickBan';
	import type { RealtimeConnectionState } from '$lib/realtime/types';
	import { formatSeriesRules } from '$lib/workflow/seriesRules';
	import type { PageProps } from './$types';

	type SeriesMutation =
		'draft' | 'submit' | 'earlyComplete' | 'revertLastGame' | 'revert' | 'delete';
	type SnapshotRefreshSource = 'manual' | 'realtime';
	const defaultRealtimeOfflineMessage = '변경 알림을 받을 수 없습니다.';
	type PickBanRoute = {
		key: string;
		guildId: string;
		seriesId: string;
	};

	let { data }: PageProps = $props();
	let snapshot = $state<SeriesPickBanSnapshot | null>(null);
	let loading = $state(true);
	let loadError = $state('');
	let mutation = $state<SeriesMutation | null>(null);
	let mutationError = $state('');
	let mutationNotice = $state('');
	let recoveryRequired = $state(false);
	let recoveringAuthoritativeSnapshot = $state(false);
	let revertDialogOpen = $state(false);
	let revertError = $state('');
	let revertLastGameDialogOpen = $state(false);
	let revertLastGameError = $state('');
	let earlyCompleteDialogOpen = $state(false);
	let earlyCompleteError = $state('');
	let earlyCompleteOutcome = $state<SeriesEarlyCompletionOutcome | null>(null);
	let deleteDialogOpen = $state(false);
	let deleteError = $state('');
	let deleteConfirmationInput = $state('');
	let activeRouteKey = $state<string | null>(null);
	let latestSnapshotRequest = 0;
	let realtimeConnectionState = $state<RealtimeConnectionState>('offline');
	let realtimeConflict = $state(false);
	let realtimeRefreshPending = $state(false);
	let realtimeOfflineMessage = $state(defaultRealtimeOfflineMessage);
	let pickBanRealtimeSocket: BrowserRealtimeSocket | null = null;
	const idempotencyKeys = createOperationIdempotencyKeys();

	const isMutating = $derived(mutation !== null);
	const mutationLocked = $derived(isMutating || recoveryRequired);
	const canEdit = $derived(Boolean(snapshot?.capabilities.canEdit));
	const canSubmit = $derived(Boolean(snapshot?.capabilities.canSubmit));
	const canEarlyComplete = $derived(Boolean(snapshot?.capabilities.canEarlyComplete));
	const canRevertLastGame = $derived(Boolean(snapshot?.capabilities.canRevertLastGame));
	const canRevert = $derived(Boolean(snapshot?.capabilities.canRevert));
	const canDelete = $derived(Boolean(snapshot?.capabilities.canDelete));
	const deleteConfirmationText = $derived(
		snapshot ? `시리즈 #${snapshot.series.displayNumber} 취소` : ''
	);
	const deleteConfirmationValid = $derived(
		deleteConfirmationInput.trim() === deleteConfirmationText
	);
	const realtimeSaveState = $derived(
		realtimeConflict ? 'conflict' : isMutating ? 'saving' : 'saved'
	);
	const realtimeResourceLabel = $derived(
		snapshot ? `${snapshot.series.title} 픽밴` : '픽밴 작업대'
	);
	const seriesDescription = $derived(
		snapshot
			? `${snapshot.series.title} · ${formatSeriesRules(snapshot.series.bestOf, snapshot.series.hardFearless)} · ${snapshot.currentGame.setNumber}세트`
			: `시리즈 #${data.seriesId} 정보를 불러오는 중`
	);

	$effect(() => {
		const route = currentPickBanRoute();
		if (route.key === activeRouteKey) return;

		activeRouteKey = route.key;
		resetRouteState();
		void loadPickBan(route);
	});

	$effect(() => {
		const route = currentPickBanRoute();
		if (!canEdit) {
			realtimeConnectionState = 'offline';
			return;
		}

		const topics = seriesPickBanRealtimeTopics(route.guildId, route.seriesId);
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

		pickBanRealtimeSocket = client;
		client.connect();
		return () => {
			if (pickBanRealtimeSocket === client) pickBanRealtimeSocket = null;
			client.disconnect();
		};
	});

	onMount(() => {
		const refreshWhenVisible = () => {
			if (globalThis.document.visibilityState !== 'visible' || !pickBanRealtimeSocket) return;
			queueRealtimeSnapshotRefresh(currentPickBanRoute());
		};
		globalThis.document.addEventListener('visibilitychange', refreshWhenVisible);
		return () => globalThis.document.removeEventListener('visibilitychange', refreshWhenVisible);
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

	function beginSnapshotRequest(): number {
		latestSnapshotRequest += 1;
		return latestSnapshotRequest;
	}

	function isLatestSnapshotRequest(route: PickBanRoute, requestNumber: number): boolean {
		return isCurrentRoute(route) && requestNumber === latestSnapshotRequest;
	}

	function isRouteInView(route: PickBanRoute): boolean {
		return currentPickBanRoute().key === route.key;
	}

	function resetRouteState() {
		latestSnapshotRequest += 1;
		snapshot = null;
		loading = true;
		loadError = '';
		mutation = null;
		mutationError = '';
		mutationNotice = '';
		recoveryRequired = false;
		recoveringAuthoritativeSnapshot = false;
		revertDialogOpen = false;
		revertError = '';
		revertLastGameDialogOpen = false;
		revertLastGameError = '';
		earlyCompleteDialogOpen = false;
		earlyCompleteError = '';
		earlyCompleteOutcome = null;
		deleteDialogOpen = false;
		deleteError = '';
		realtimeConnectionState = 'offline';
		realtimeConflict = false;
		realtimeRefreshPending = false;
		realtimeOfflineMessage = defaultRealtimeOfflineMessage;
		idempotencyKeys.invalidate('series-submit');
		idempotencyKeys.invalidate('series-early-complete');
		idempotencyKeys.invalidate('series-revert-last-game');
		idempotencyKeys.invalidate('series-revert');
		idempotencyKeys.invalidate('series-delete');
	}

	function applyAuthoritativeSnapshot(nextSnapshot: SeriesPickBanSnapshot) {
		snapshot = nextSnapshot;
	}

	async function redirectToCompletedSeries(cause: unknown, route: PickBanRoute): Promise<boolean> {
		if (!(cause instanceof SeriesPickBanApiError) || cause.code !== 'series_not_in_progress') {
			return false;
		}

		try {
			await fetchSeriesResult(route.guildId, route.seriesId);
		} catch {
			return false;
		}

		if (!isRouteInView(route)) return true;
		await goto(
			resolve('/g/[guildId]/series/[id]/result', {
				guildId: route.guildId,
				id: route.seriesId
			})
		);
		return true;
	}

	async function loadPickBan(route = currentPickBanRoute()) {
		if (!isCurrentRoute(route)) return;
		const requestNumber = beginSnapshotRequest();
		loading = true;
		loadError = '';
		try {
			const nextSnapshot = await fetchSeriesPickBan(route.guildId, route.seriesId);
			if (!isLatestSnapshotRequest(route, requestNumber)) return;
			applyAuthoritativeSnapshot(nextSnapshot);
			recoveryRequired = false;
			realtimeConflict = false;
		} catch (cause) {
			if (!isLatestSnapshotRequest(route, requestNumber)) return;
			if (await redirectToCompletedSeries(cause, route)) return;
			if (!isLatestSnapshotRequest(route, requestNumber)) return;
			loadError = seriesPickBanErrorText(cause);
		} finally {
			if (isLatestSnapshotRequest(route, requestNumber)) loading = false;
		}
	}

	async function recoverAuthoritativeSnapshot(
		conflictMessage: string | null,
		route = currentPickBanRoute(),
		source: SnapshotRefreshSource = 'manual'
	) {
		if (!isCurrentRoute(route) || recoveringAuthoritativeSnapshot) return;

		recoveryRequired = true;
		recoveringAuthoritativeSnapshot = true;
		const requestNumber = beginSnapshotRequest();
		try {
			const nextSnapshot = await fetchSeriesPickBan(route.guildId, route.seriesId);
			if (!isLatestSnapshotRequest(route, requestNumber)) return;
			applyAuthoritativeSnapshot(nextSnapshot);
			recoveryRequired = false;
			realtimeConflict = false;
			if (source === 'manual') {
				mutationError = '';
				mutationNotice = conflictMessage
					? `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경된 픽밴을 확인한 뒤 다시 시도해 주세요.`
					: '서버 최신 상태를 불러왔습니다. 변경을 다시 시도할 수 있습니다.';
			}
		} catch (refreshCause) {
			if (!isLatestSnapshotRequest(route, requestNumber)) return;
			if (await redirectToCompletedSeries(refreshCause, route)) return;
			if (!isLatestSnapshotRequest(route, requestNumber)) return;
			const refreshMessage = seriesPickBanErrorText(refreshCause);
			realtimeConflict = true;
			if (source === 'manual') {
				mutationError = conflictMessage
					? `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${refreshMessage}`
					: `최신 상태를 불러오지 못했습니다. ${refreshMessage}`;
			}
		} finally {
			if (isCurrentRoute(route)) {
				recoveringAuthoritativeSnapshot = false;
				if (requestNumber === latestSnapshotRequest) {
					void flushRealtimeSnapshotRefresh(route);
				}
			}
		}
	}

	function handleRealtimeInvalidation(
		event: RealtimeInvalidateEvent,
		route: PickBanRoute,
		topics: SeriesPickBanRealtimeTopics
	) {
		if (!isRouteInView(route) || !snapshot) return;
		if (!isNewerSeriesPickBanInvalidation(event, snapshot, topics)) return;
		queueRealtimeSnapshotRefresh(route);
	}

	function queueRealtimeSnapshotRefresh(route: PickBanRoute) {
		if (!isCurrentRoute(route) || !snapshot) return;
		realtimeRefreshPending = true;
		void flushRealtimeSnapshotRefresh(route);
	}

	async function flushRealtimeSnapshotRefresh(route: PickBanRoute) {
		if (!isCurrentRoute(route) || !realtimeRefreshPending) return;
		if (isMutating || recoveringAuthoritativeSnapshot) return;

		realtimeRefreshPending = false;
		await recoverAuthoritativeSnapshot(null, route, 'realtime');
	}

	function reconnectRealtime() {
		realtimeOfflineMessage = defaultRealtimeOfflineMessage;
		pickBanRealtimeSocket?.reconnect();
	}

	function invalidateIdempotencyKey(operation: SeriesMutation) {
		if (operation === 'submit') idempotencyKeys.invalidate('series-submit');
		if (operation === 'earlyComplete') idempotencyKeys.invalidate('series-early-complete');
		if (operation === 'revertLastGame') idempotencyKeys.invalidate('series-revert-last-game');
		if (operation === 'revert') idempotencyKeys.invalidate('series-revert');
		if (operation === 'delete') idempotencyKeys.invalidate('series-delete');
	}

	async function recoverAfterRevisionConflict(
		cause: unknown,
		operation: SeriesMutation,
		route: PickBanRoute
	) {
		invalidateIdempotencyKey(operation);
		realtimeConflict = true;
		await recoverAuthoritativeSnapshot(seriesPickBanErrorText(cause), route);
	}

	async function retryAuthoritativeSnapshot() {
		mutationError = '';
		mutationNotice = '';
		await recoverAuthoritativeSnapshot(null);
	}

	async function applyDraftOperation(operation: SeriesPickBanDraftOperation) {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canEdit || mutationLocked) return;

		mutation = 'draft';
		mutationError = '';
		mutationNotice = '';
		try {
			const nextSnapshot = await updateSeriesPickBanDraft(route.guildId, route.seriesId, {
				expectedRevision: currentSnapshot.series.revision,
				operation
			});
			if (!isCurrentRoute(route)) return;
			applyAuthoritativeSnapshot(nextSnapshot);
			recoveryRequired = false;
			realtimeConflict = false;
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isSeriesPickBanRevisionConflict(cause)) {
				await recoverAfterRevisionConflict(cause, 'draft', route);
			} else {
				mutationError = seriesPickBanErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function submitCurrentGame(game: {
		sidePreset?: 'DEFAULT' | 'SWAPPED';
		winner: 'BLUE' | 'RED' | null;
		picks: Record<string, string | null>;
		bans: { BLUE: Array<string | null>; RED: Array<string | null> };
	}) {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (
			!isCurrentRoute(route) ||
			!currentSnapshot ||
			!canSubmit ||
			!game.winner ||
			mutationLocked
		) {
			return;
		}

		const operation = {
			setNumber: currentSnapshot.currentGame.setNumber,
			expectedRevision: currentSnapshot.series.revision,
			sidePreset: game.sidePreset ?? currentSnapshot.currentGame.sidePreset,
			winner: game.winner,
			picks: game.picks,
			bans: game.bans,
			patchVersion: currentSnapshot.currentGame.patchVersion ?? data.championCatalog.version
		};
		const idempotencyKey = idempotencyKeys.keyFor('series-submit', operation);
		mutation = 'submit';
		mutationError = '';
		mutationNotice = '';
		try {
			const result = await submitSeriesPickBanGame(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-submit');
			const pickBanHref = resolve('/g/[guildId]/series/[id]/pick-ban', {
				guildId: route.guildId,
				id: route.seriesId
			});
			if (result.nextHref === pickBanHref) {
				await loadPickBan(route);
			} else {
				await goto(result.nextHref);
			}
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isSeriesPickBanRevisionConflict(cause)) {
				await recoverAfterRevisionConflict(cause, 'submit', route);
			} else {
				mutationError = seriesPickBanErrorText(cause);
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	function openRevertDialog() {
		if (!snapshot || !canRevert || mutationLocked) return;
		revertLastGameDialogOpen = false;
		earlyCompleteDialogOpen = false;
		deleteDialogOpen = false;
		revertError = '';
		mutationError = '';
		revertDialogOpen = true;
	}

	function openDeleteDialog() {
		if (!snapshot || !canDelete || mutationLocked) return;
		revertDialogOpen = false;
		revertLastGameDialogOpen = false;
		earlyCompleteDialogOpen = false;
		deleteError = '';
		deleteConfirmationInput = '';
		mutationError = '';
		deleteDialogOpen = true;
	}

	function openRevertLastGameDialog() {
		if (!snapshot || !canRevertLastGame || mutationLocked) return;
		revertDialogOpen = false;
		earlyCompleteDialogOpen = false;
		deleteDialogOpen = false;
		revertLastGameError = '';
		mutationError = '';
		revertLastGameDialogOpen = true;
	}

	function openEarlyCompleteDialog() {
		if (!snapshot || !canEarlyComplete || mutationLocked) return;
		const score = seriesScore(snapshot);
		revertDialogOpen = false;
		revertLastGameDialogOpen = false;
		deleteDialogOpen = false;
		earlyCompleteError = '';
		mutationError = '';
		earlyCompleteOutcome =
			score.team1 === score.team2 ? 'DRAW' : score.team1 > score.team2 ? 'TEAM_1' : 'TEAM_2';
		earlyCompleteDialogOpen = true;
	}

	async function confirmEarlyComplete() {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		const outcome = earlyCompleteOutcome;
		if (
			!isCurrentRoute(route) ||
			!currentSnapshot ||
			!canEarlyComplete ||
			!outcome ||
			mutationLocked
		) {
			return;
		}

		const operation = { expectedRevision: currentSnapshot.series.revision, outcome };
		const idempotencyKey = idempotencyKeys.keyFor('series-early-complete', operation);
		mutation = 'earlyComplete';
		mutationError = '';
		earlyCompleteError = '';
		try {
			const result = await completeSeriesEarly(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-early-complete');
			earlyCompleteDialogOpen = false;
			await goto(result.nextHref);
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isSeriesPickBanRevisionConflict(cause)) {
				earlyCompleteDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'earlyComplete', route);
			} else {
				earlyCompleteError = seriesPickBanErrorText(cause);
				mutationError = earlyCompleteError;
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function confirmRevertLastGame() {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canRevertLastGame || mutationLocked) {
			return;
		}

		const operation = { expectedRevision: currentSnapshot.series.revision };
		const idempotencyKey = idempotencyKeys.keyFor('series-revert-last-game', operation);
		mutation = 'revertLastGame';
		mutationError = '';
		revertLastGameError = '';
		try {
			const result = await revertLastSeriesPickBanGame(route.guildId, route.seriesId, {
				...operation,
				idempotencyKey
			});
			if (!isCurrentRoute(route)) return;
			idempotencyKeys.complete('series-revert-last-game');
			const pickBanHref = resolve('/g/[guildId]/series/[id]/pick-ban', {
				guildId: route.guildId,
				id: route.seriesId
			});
			if (result.nextHref === pickBanHref) {
				revertLastGameDialogOpen = false;
				await loadPickBan(route);
			} else {
				await goto(result.nextHref);
			}
		} catch (cause) {
			if (!isCurrentRoute(route)) return;
			if (isSeriesPickBanRevisionConflict(cause)) {
				revertLastGameDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'revertLastGame', route);
			} else {
				revertLastGameError = seriesPickBanErrorText(cause);
				mutationError = revertLastGameError;
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function confirmRevert() {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (!isCurrentRoute(route) || !currentSnapshot || !canRevert || mutationLocked) return;

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
			if (isSeriesPickBanRevisionConflict(cause)) {
				revertDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'revert', route);
			} else {
				revertError = seriesPickBanErrorText(cause);
				mutationError = revertError;
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function confirmDelete() {
		const route = currentPickBanRoute();
		const currentSnapshot = snapshot;
		if (
			!isCurrentRoute(route) ||
			!currentSnapshot ||
			!canDelete ||
			!deleteConfirmationValid ||
			mutationLocked
		)
			return;

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
			if (isSeriesPickBanRevisionConflict(cause)) {
				deleteDialogOpen = false;
				await recoverAfterRevisionConflict(cause, 'delete', route);
			} else {
				deleteError = seriesPickBanErrorText(cause);
				mutationError = deleteError;
			}
		} finally {
			if (isCurrentRoute(route)) {
				mutation = null;
				void flushRealtimeSnapshotRefresh(route);
			}
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

	function boardTeamsFor(
		snapshot: SeriesPickBanSnapshot,
		sidePreset = snapshot.currentGame.sidePreset
	) {
		const teamOneSide = sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
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
			sideSelected: snapshot.currentGame.sideSelected,
			winner: snapshot.currentGame.winner,
			bans: {
				BLUE: [...snapshot.currentGame.bans.BLUE],
				RED: [...snapshot.currentGame.bans.RED]
			},
			picks: { ...snapshot.currentGame.picks }
		};
	}

	function previousPickChampionIds(snapshot: SeriesPickBanSnapshot): string[] {
		return snapshot.games.flatMap((game) =>
			Object.values(game.picks).filter((championId): championId is string => championId !== null)
		);
	}

	function seriesScore(snapshot: SeriesPickBanSnapshot) {
		return snapshot.games.reduce(
			(score, game) => {
				const teamOneWon =
					(game.sidePreset === 'DEFAULT' && game.winner === 'BLUE') ||
					(game.sidePreset === 'SWAPPED' && game.winner === 'RED');
				if (teamOneWon) score.team1 += 1;
				else score.team2 += 1;
				return score;
			},
			{ team1: 0, team2: 0 }
		);
	}

	function earlyCompleteOutcomeLabel(
		snapshot: SeriesPickBanSnapshot,
		outcome: SeriesEarlyCompletionOutcome
	): string {
		if (outcome === 'DRAW') return '무승부';
		const team = snapshot.teams.find((candidate) => candidate.team === outcome);
		if (!team) throw new Error(`Missing ${outcome} in a validated pick-ban snapshot`);
		return `${team.name} 승리`;
	}
</script>

<svelte:head>
	<title>픽밴 작업대 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader kicker="일반 내전" title="픽밴 작업대" description={seriesDescription}>
		{#snippet actions()}
			<div class="flex flex-wrap items-center justify-end gap-2">
				{#if canRevert}
					<button
						type="button"
						disabled={mutationLocked}
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
				{#if canRevertLastGame}
					<button
						type="button"
						disabled={mutationLocked}
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
				{#if canEarlyComplete}
					<button
						type="button"
						disabled={mutationLocked}
						onclick={openEarlyCompleteDialog}
						class="inline-flex h-10 items-center gap-1.5 border border-amber-300/30 bg-amber-400/[0.06] px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/[0.12] disabled:cursor-not-allowed disabled:opacity-45"
					>
						{#if mutation === 'earlyComplete'}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 종료 중
						{:else}
							<Flag size={14} aria-hidden="true" /> 조기 종료
						{/if}
					</button>
				{/if}
				{#if canDelete}
					<button
						type="button"
						disabled={mutationLocked}
						onclick={openDeleteDialog}
						class="grid size-10 place-items-center border border-rose-300/25 text-rose-200 enabled:hover:bg-rose-400/10 disabled:cursor-not-allowed disabled:opacity-45"
						aria-label="시리즈 취소"
						title="시리즈 취소"
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

	{#if canEdit}
		<RealtimeEditStateBar
			resourceLabel={realtimeResourceLabel}
			connectionState={realtimeConnectionState}
			saveState={realtimeSaveState}
			showEditors={false}
			offlineMessage={realtimeOfflineMessage}
			onReconnect={reconnectRealtime}
			onAcceptServer={() => void retryAuthoritativeSnapshot()}
		/>
	{/if}
	{#if snapshot}
		<ScrimPickBanConfirmedSetsDisclosure {snapshot} catalog={data.championCatalog} />
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
			class="border border-cyan-300/20 bg-cyan-400/[0.055] px-3 py-2 text-xs leading-5 font-semibold break-words text-cyan-100"
			aria-live="polite"
		>
			{mutationNotice}
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
		{#key `${snapshot.series.id}:${snapshot.currentGame.setNumber}`}
			<ScrimPickBanResultInputBoard
				teams={boardTeams}
				teamsAreGameSides
				hardFearless={snapshot.series.hardFearless}
				previousPickChampionIds={previousPickChampionIds(snapshot)}
				readOnly={!canEdit}
				busy={mutationLocked}
				initialGame={boardGame}
				champions={data.championCatalog.champions}
				patchVersion={snapshot.currentGame.patchVersion ?? data.championCatalog.version}
				onDraftOperation={canEdit ? applyDraftOperation : undefined}
				onSubmit={canSubmit ? submitCurrentGame : undefined}
			/>
		{/key}
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
					시리즈를 엔트리 편성으로 되돌립니다. 확정된 게임 결과와 해당 게임의 MMR 반영도 함께
					되돌려집니다.
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
					disabled={mutationLocked}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="마지막 결과 되돌리기 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					가장 최근 확정 게임을 취소하고 해당 게임의 MMR 반영과 시리즈 점수를 되돌립니다. 이 작업은
					즉시 적용됩니다.
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
					disabled={mutationLocked}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={mutationLocked}
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

<Dialog.Root bind:open={earlyCompleteDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">시리즈 조기 종료</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="시리즈 조기 종료 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			{#if snapshot}
				{@const score = seriesScore(snapshot)}
				<div class="grid gap-4 p-4">
					<Dialog.Description class="text-xs leading-5 text-slate-400">
						확정된 {snapshot.games.length}세트와 MMR 정산은 유지됩니다. 현재
						{snapshot.currentGame.setNumber}세트에 입력한 픽밴은 결과에 포함되지 않습니다.
					</Dialog.Description>
					<div
						class="flex items-center justify-between border border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] px-3 py-2"
					>
						<span class="text-xs font-medium text-slate-400">현재 스코어</span>
						<strong class="text-sm tabular-nums text-slate-100"
							>{score.team1} : {score.team2}</strong
						>
					</div>
					<div class="grid gap-2" role="group" aria-label="조기 종료 결과 선택">
						<span class="text-xs font-semibold text-slate-200">최종 처리</span>
						<div class="grid grid-cols-3 gap-2">
							<button
								type="button"
								disabled={isMutating}
								aria-pressed={earlyCompleteOutcome === 'TEAM_1'}
								onclick={() => (earlyCompleteOutcome = 'TEAM_1')}
								class={`min-h-12 border px-2 text-xs font-semibold transition-colors disabled:opacity-40 ${
									earlyCompleteOutcome === 'TEAM_1'
										? 'border-cyan-300/50 bg-cyan-400/10 text-cyan-100'
										: 'border-[var(--color-app-border)] text-slate-400 hover:bg-[var(--color-app-hover)]'
								}`}
							>
								{earlyCompleteOutcomeLabel(snapshot, 'TEAM_1')}
							</button>
							<button
								type="button"
								disabled={isMutating || score.team1 !== score.team2}
								aria-pressed={earlyCompleteOutcome === 'DRAW'}
								title={score.team1 === score.team2
									? undefined
									: '동점일 때만 무승부 처리할 수 있습니다.'}
								onclick={() => (earlyCompleteOutcome = 'DRAW')}
								class={`min-h-12 border px-2 text-xs font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
									earlyCompleteOutcome === 'DRAW'
										? 'border-amber-300/50 bg-amber-400/10 text-amber-100'
										: 'border-[var(--color-app-border)] text-slate-400 hover:bg-[var(--color-app-hover)]'
								}`}
							>
								무승부
							</button>
							<button
								type="button"
								disabled={isMutating}
								aria-pressed={earlyCompleteOutcome === 'TEAM_2'}
								onclick={() => (earlyCompleteOutcome = 'TEAM_2')}
								class={`min-h-12 border px-2 text-xs font-semibold transition-colors disabled:opacity-40 ${
									earlyCompleteOutcome === 'TEAM_2'
										? 'border-rose-300/50 bg-rose-400/10 text-rose-100'
										: 'border-[var(--color-app-border)] text-slate-400 hover:bg-[var(--color-app-hover)]'
								}`}
							>
								{earlyCompleteOutcomeLabel(snapshot, 'TEAM_2')}
							</button>
						</div>
					</div>
					{#if earlyCompleteError}
						<p
							class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
							role="alert"
						>
							{earlyCompleteError}
						</p>
					{/if}
				</div>
			{/if}
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={isMutating}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={isMutating || !earlyCompleteOutcome}
					onclick={confirmEarlyComplete}
					class="inline-flex h-9 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 disabled:opacity-35"
				>
					{#if mutation === 'earlyComplete'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 종료 중
					{:else}
						<Flag size={14} aria-hidden="true" /> 조기 종료 확정
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
				<Dialog.Title class="text-sm font-semibold text-slate-50">시리즈 취소</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="시리즈 취소 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					확정된 게임 결과와 MMR 반영을 안전하게 역순으로 되돌린 뒤 시리즈를 취소합니다. 원본 모집과
					엔트리는 보존되며 취소 후 복구 화면으로 이동합니다.
				</Dialog.Description>
				<label class="grid gap-1.5 text-xs font-semibold text-slate-300">
					<span>확인 문구</span>
					<input
						bind:value={deleteConfirmationInput}
						disabled={isMutating}
						placeholder={deleteConfirmationText}
						class="h-9 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-medium text-slate-100 outline-none focus:border-rose-300/50 disabled:opacity-35"
					/>
				</label>
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
					disabled={isMutating || !deleteConfirmationValid}
					onclick={confirmDelete}
					class="inline-flex h-9 items-center gap-1.5 border border-rose-300/35 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 disabled:opacity-35"
				>
					{#if mutation === 'delete'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 취소 중
					{:else}
						<Trash2 size={14} aria-hidden="true" /> 시리즈 취소
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
