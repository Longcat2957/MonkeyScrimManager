<script lang="ts">
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { Flag, LoaderCircle, X } from 'lucide-svelte';
	import {
		assignAuctionMatchLineup,
		auctionMatchLineupErrorText,
		isAuctionMatchLineupRevisionConflict,
		lockAuctionMatchLineup,
		unlockAuctionMatchLineup
	} from '$lib/api/auctionMatchLineup';
	import {
		auctionMatchGameResultErrorText,
		completeAuctionFinalAsBo1,
		fetchAuctionMatchGameResult,
		isAuctionMatchGameResultRevisionConflict,
		revertLastAuctionMatchGameResult,
		submitAuctionMatchGameResult
	} from '$lib/api/auctionMatchGameResult';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionMatchGameResultInputBoard from '$lib/components/workflow/AuctionMatchGameResultInputBoard.svelte';
	import AuctionMatchStartingLineupInputBoard from '$lib/components/workflow/AuctionMatchStartingLineupInputBoard.svelte';
	import {
		auctionMatchGameResultSnapshotSchema,
		type AuctionMatchGameResultRevertCommand,
		type AuctionMatchGameResultSnapshot,
		type AuctionMatchGameResultSubmitCommand
	} from '$lib/contracts/auctionMatchGameResult';
	import type {
		AuctionMatchLineupAssignCommand,
		AuctionMatchLineupLockCommand,
		AuctionMatchLineupUnlockCommand
	} from '$lib/contracts/auctionMatchLineup';
	import type { PageProps } from './$types';

	type MatchMutation =
		'assign' | 'lock' | 'unlock' | 'submit_game' | 'revert_game' | 'complete_bo1';

	const invalidSnapshotMessage = '경매 경기 응답 형식이 올바르지 않습니다.';

	let { data }: PageProps = $props();

	let snapshotOverride = $state<AuctionMatchGameResultSnapshot | null>(null);
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let mutation = $state<MatchMutation | null>(null);
	let mutationError = $state('');
	let completeBo1DialogOpen = $state(false);
	let idempotencyKeys = createOperationIdempotencyKeys();

	const incomingSnapshotKey = $derived(`${data.snapshot.match.id}:${data.snapshot.match.revision}`);
	const snapshot = $derived(
		snapshotOverride !== null && snapshotOverrideSourceKey === incomingSnapshotKey
			? snapshotOverride
			: data.snapshot
	);
	const match = $derived(snapshot.match);
	const busy = $derived(mutation !== null);
	const statusLabel = $derived(matchStatusLabel(match.status));
	const currentLeaderName = $derived(
		snapshot.teams.find(
			(team) =>
				team.id === (match.leftScore > match.rightScore ? match.leftTeamId : match.rightTeamId)
		)?.teamName ?? '현재 선두 팀'
	);

	function parseAuthoritativeSnapshot(value: unknown): AuctionMatchGameResultSnapshot {
		const parsed = auctionMatchGameResultSnapshotSchema.safeParse(value);
		if (!parsed.success) throw new Error(invalidSnapshotMessage);
		return parsed.data;
	}

	function applySnapshot(nextSnapshot: AuctionMatchGameResultSnapshot) {
		snapshotOverrideSourceKey = incomingSnapshotKey;
		snapshotOverride = nextSnapshot;
	}

	async function recoverAuthoritativeSnapshot(conflictMessage: string) {
		try {
			const nextSnapshot = await fetchAuctionMatchGameResult(data.guildId, match.id);
			applySnapshot(nextSnapshot);
			mutationError = `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
		} catch (refreshCause) {
			mutationError = `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${auctionMatchGameResultErrorText(refreshCause)}`;
		}
	}

	function statusAllowsLineupMutation(): boolean {
		return (
			snapshot.permissions.canManageLineup &&
			!snapshot.match.lineupsLocked &&
			snapshot.match.status === 'LINEUP_PENDING'
		);
	}

	function statusAllowsLineupUnlock(): boolean {
		return snapshot.permissions.canUnlockLineup && snapshot.match.lineupsLocked;
	}

	function statusAllowsGameResultMutation(): boolean {
		const requiredWins = Math.floor(snapshot.match.bestOf / 2) + 1;
		return (
			snapshot.permissions.canEnterResults &&
			snapshot.match.lineupsLocked &&
			snapshot.match.status !== 'COMPLETED' &&
			snapshot.match.status !== 'CANCELLED' &&
			snapshot.games.length < snapshot.match.bestOf &&
			snapshot.match.leftScore < requiredWins &&
			snapshot.match.rightScore < requiredWins
		);
	}

	function statusAllowsGameResultRevert(): boolean {
		return snapshot.games.length > 0 && snapshot.permissions.canRevertLastGame;
	}

	function lineupMutationErrorText(cause: unknown): string {
		return cause instanceof Error && cause.message === invalidSnapshotMessage
			? invalidSnapshotMessage
			: auctionMatchLineupErrorText(cause);
	}

	function gameMutationErrorText(cause: unknown): string {
		return cause instanceof Error && cause.message === invalidSnapshotMessage
			? invalidSnapshotMessage
			: auctionMatchGameResultErrorText(cause);
	}

	async function handleAssign(command: AuctionMatchLineupAssignCommand): Promise<void> {
		if (busy || !statusAllowsLineupMutation()) return;
		if (command.expectedRevision !== snapshot.match.revision) {
			await recoverAuthoritativeSnapshot('라인업 버전이 변경되었습니다.');
			throw new Error('stale_auction_match_lineup_assignment');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-match-lineup-assign', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		mutation = 'assign';
		mutationError = '';
		try {
			const nextSnapshot = parseAuthoritativeSnapshot(
				await assignAuctionMatchLineup(data.guildId, match.id, command, idempotencyKey)
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-match-lineup-assign');
		} catch (cause) {
			if (isAuctionMatchLineupRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-match-lineup-assign');
				await recoverAuthoritativeSnapshot(auctionMatchLineupErrorText(cause));
			} else {
				mutationError = lineupMutationErrorText(cause);
			}
			throw cause;
		} finally {
			mutation = null;
		}
	}

	async function handleLock(command: AuctionMatchLineupLockCommand): Promise<void> {
		if (busy || !statusAllowsLineupMutation()) return;
		if (command.expectedRevision !== snapshot.match.revision) {
			await recoverAuthoritativeSnapshot('라인업 버전이 변경되었습니다.');
			throw new Error('stale_auction_match_lineup_lock');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-match-lineup-lock', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		mutation = 'lock';
		mutationError = '';
		try {
			const nextSnapshot = parseAuthoritativeSnapshot(
				await lockAuctionMatchLineup(data.guildId, match.id, command, idempotencyKey)
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-match-lineup-lock');
		} catch (cause) {
			if (isAuctionMatchLineupRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-match-lineup-lock');
				await recoverAuthoritativeSnapshot(auctionMatchLineupErrorText(cause));
			} else {
				mutationError = lineupMutationErrorText(cause);
			}
			throw cause;
		} finally {
			mutation = null;
		}
	}

	async function handleUnlock(command: AuctionMatchLineupUnlockCommand): Promise<void> {
		if (busy || !statusAllowsLineupUnlock()) return;
		if (command.expectedRevision !== snapshot.match.revision) {
			await recoverAuthoritativeSnapshot('라인업 버전이 변경되었습니다.');
			throw new Error('stale_auction_match_lineup_unlock');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-match-lineup-unlock', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		mutation = 'unlock';
		mutationError = '';
		try {
			const nextSnapshot = parseAuthoritativeSnapshot(
				await unlockAuctionMatchLineup(data.guildId, match.id, command, idempotencyKey)
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-match-lineup-unlock');
		} catch (cause) {
			if (isAuctionMatchLineupRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-match-lineup-unlock');
				await recoverAuthoritativeSnapshot(auctionMatchLineupErrorText(cause));
			} else {
				mutationError = lineupMutationErrorText(cause);
			}
			throw cause;
		} finally {
			mutation = null;
		}
	}

	async function handleSubmitGame(command: AuctionMatchGameResultSubmitCommand): Promise<void> {
		if (busy || !statusAllowsGameResultMutation()) return;
		if (command.expectedRevision !== snapshot.match.revision) {
			await recoverAuthoritativeSnapshot('경기 버전이 변경되었습니다.');
			throw new Error('stale_auction_match_game_submission');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-match-game-submit', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		mutation = 'submit_game';
		mutationError = '';
		try {
			const nextSnapshot = await submitAuctionMatchGameResult(
				data.guildId,
				match.id,
				command,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-match-game-submit');
		} catch (cause) {
			if (isAuctionMatchGameResultRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-match-game-submit');
				await recoverAuthoritativeSnapshot(auctionMatchGameResultErrorText(cause));
			} else {
				mutationError = gameMutationErrorText(cause);
			}
			throw cause;
		} finally {
			mutation = null;
		}
	}

	async function handleDeleteLastGame(command: AuctionMatchGameResultRevertCommand): Promise<void> {
		if (busy || !statusAllowsGameResultRevert()) return;
		if (command.expectedRevision !== snapshot.match.revision) {
			await recoverAuthoritativeSnapshot('경기 버전이 변경되었습니다.');
			throw new Error('stale_auction_match_game_revert');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-match-game-revert', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		mutation = 'revert_game';
		mutationError = '';
		try {
			const nextSnapshot = await revertLastAuctionMatchGameResult(
				data.guildId,
				match.id,
				command,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-match-game-revert');
		} catch (cause) {
			if (isAuctionMatchGameResultRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-match-game-revert');
				await recoverAuthoritativeSnapshot(auctionMatchGameResultErrorText(cause));
			} else {
				mutationError = gameMutationErrorText(cause);
			}
			throw cause;
		} finally {
			mutation = null;
		}
	}

	async function handleCompleteFinalAsBo1(): Promise<void> {
		if (busy || !snapshot.permissions.canCompleteFinalAsBo1) return;

		const command = { expectedRevision: snapshot.match.revision };
		const idempotencyKey = idempotencyKeys.keyFor('auction-final-complete-bo1', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		mutation = 'complete_bo1';
		mutationError = '';
		try {
			const nextSnapshot = await completeAuctionFinalAsBo1(
				data.guildId,
				match.id,
				command,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-final-complete-bo1');
			completeBo1DialogOpen = false;
		} catch (cause) {
			completeBo1DialogOpen = false;
			if (isAuctionMatchGameResultRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-final-complete-bo1');
				await recoverAuthoritativeSnapshot(auctionMatchGameResultErrorText(cause));
			} else {
				mutationError = gameMutationErrorText(cause);
			}
		} finally {
			mutation = null;
		}
	}

	function matchStatusLabel(status: AuctionMatchGameResultSnapshot['match']['status']): string {
		if (status === 'LINEUP_PENDING') return '라인업 편성 중';
		if (status === 'READY') return '시작 준비';
		if (status === 'IN_PROGRESS') return '진행 중';
		if (status === 'COMPLETED') return '완료';
		return '취소됨';
	}
</script>

<svelte:head>
	<title>{match.label} | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title={match.label}
		description={`BO${match.bestOf} · ${match.hardFearless ? '하드 피어리스' : '일반'} · ${statusLabel}`}
	>
		{#snippet actions()}
			{#if snapshot.permissions.canCompleteFinalAsBo1}
				<button
					type="button"
					disabled={busy}
					onclick={() => (completeBo1DialogOpen = true)}
					class="inline-flex h-9 shrink-0 items-center gap-1.5 justify-center border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/15 disabled:opacity-35"
				>
					<Flag size={14} aria-hidden="true" /> BO1로 조기 종료
				</button>
			{/if}
			{#if match.status === 'READY' || match.status === 'IN_PROGRESS' || match.status === 'COMPLETED'}
				<a
					href={resolve('/g/[guildId]/auction/tournaments/[id]/result', {
						guildId: data.guildId,
						id: match.tournamentId
					})}
					class="inline-flex h-9 shrink-0 items-center justify-center border border-emerald-300/30 px-3 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/10"
				>
					{match.status === 'COMPLETED' ? '토너먼트 결과' : '토너먼트 현황'}
				</a>
			{/if}
			{#if match.status === 'LINEUP_PENDING'}
				<a
					href={resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
						guildId: data.guildId,
						id: match.tournamentId
					})}
					class="inline-flex h-9 shrink-0 items-center justify-center border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
				>
					브래킷
				</a>
			{/if}
		{/snippet}
	</WorkspaceHeader>

	{#if mutationError}
		<div
			role="alert"
			class="flex min-h-10 items-center gap-2 border border-rose-300/30 bg-rose-400/10 px-3 text-xs font-medium text-rose-100"
		>
			{mutationError}
		</div>
	{/if}

	{#if busy}
		<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
			<LoaderCircle size={15} class="animate-spin" aria-hidden="true" />
			{mutation === 'assign'
				? '라인업 배치 반영 중'
				: mutation === 'lock'
					? '라인업 확정 중'
					: mutation === 'unlock'
						? '라인업 확정 해제 중'
						: mutation === 'submit_game'
							? '게임 결과 기록 중'
							: mutation === 'revert_game'
								? '마지막 게임 결과 되돌리는 중'
								: 'BO1 조기 종료 처리 중'}
		</div>
	{/if}

	<AuctionMatchStartingLineupInputBoard
		{snapshot}
		{busy}
		onAssign={handleAssign}
		onLock={handleLock}
		onUnlock={handleUnlock}
	/>

	<AuctionMatchGameResultInputBoard
		{snapshot}
		catalog={data.championCatalog}
		{busy}
		onSubmit={handleSubmitGame}
		onDeleteLastGame={handleDeleteLastGame}
	/>
</div>

<Dialog.Root bind:open={completeBo1DialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50"
					>결승을 BO1로 조기 종료</Dialog.Title
				>
				<Dialog.Close
					disabled={busy}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="BO1 조기 종료 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					확정된 첫 세트와 MMR 정산은 그대로 유지하고, 결승 규칙을 BO1로 바꿔
					<strong class="text-slate-100">{currentLeaderName}</strong>의 우승으로 완료합니다.
				</Dialog.Description>
				<p
					class="border border-amber-300/20 bg-amber-400/5 px-3 py-2 text-xs leading-5 text-amber-100"
				>
					전체 내전 취소와는 별개입니다. 준결승 및 결승 기록은 삭제하거나 되돌리지 않습니다.
				</p>
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={busy}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					닫기
				</Dialog.Close>
				<button
					type="button"
					disabled={busy || !snapshot.permissions.canCompleteFinalAsBo1}
					onclick={handleCompleteFinalAsBo1}
					class="inline-flex h-9 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 disabled:opacity-35"
				>
					{#if mutation === 'complete_bo1'}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 처리 중
					{:else}
						<Flag size={14} aria-hidden="true" /> BO1 우승 확정
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
