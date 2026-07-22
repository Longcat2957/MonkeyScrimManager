<script lang="ts">
	import { resolve } from '$app/paths';
	import { LoaderCircle } from 'lucide-svelte';
	import {
		assignAuctionMatchLineup,
		auctionMatchLineupErrorText,
		isAuctionMatchLineupRevisionConflict,
		lockAuctionMatchLineup,
		unlockAuctionMatchLineup
	} from '$lib/api/auctionMatchLineup';
	import { fetchAuctionMatchGameResult } from '$lib/api/auctionMatchGameResult';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionMatchStartingLineupInputBoard from '$lib/components/workflow/AuctionMatchStartingLineupInputBoard.svelte';
	import {
		auctionMatchGameResultSnapshotSchema,
		type AuctionMatchGameResultSnapshot
	} from '$lib/contracts/auctionMatchGameResult';
	import type {
		AuctionMatchLineupAssignCommand,
		AuctionMatchLineupLockCommand,
		AuctionMatchLineupUnlockCommand
	} from '$lib/contracts/auctionMatchLineup';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	type LineupMutation = 'assign' | 'lock' | 'unlock';

	const invalidSnapshotMessage = '경매 경기 응답 형식이 올바르지 않습니다.';

	let { data }: PageProps = $props();

	let snapshotOverrides = $state<Record<string, AuctionMatchGameResultSnapshot>>({});
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let busyMatchId = $state<string | null>(null);
	let mutation = $state<LineupMutation | null>(null);
	let mutationError = $state('');
	let idempotencyKeys = createOperationIdempotencyKeys();

	const incomingSnapshotKey = $derived(
		data.semifinalSnapshots
			.map((snapshot) => `${snapshot.match.id}:${snapshot.match.revision}`)
			.join('|')
	);
	const semifinalSnapshots = $derived(
		data.semifinalSnapshots.map((snapshot) =>
			snapshotOverrideSourceKey === incomingSnapshotKey
				? (snapshotOverrides[snapshot.match.id] ?? snapshot)
				: snapshot
		)
	);
	const allSemifinalLineupsLocked = $derived(
		semifinalSnapshots.every((snapshot) => snapshot.match.lineupsLocked)
	);
	const tournament = $derived(data.bracket.tournament);
	const rulesLabel = $derived(formatAuctionRules(tournament.format, tournament.rules));
	const busy = $derived(busyMatchId !== null);

	function parseAuthoritativeSnapshot(value: unknown): AuctionMatchGameResultSnapshot {
		const parsed = auctionMatchGameResultSnapshotSchema.safeParse(value);
		if (!parsed.success) throw new Error(invalidSnapshotMessage);
		return parsed.data;
	}

	function applySnapshot(nextSnapshot: AuctionMatchGameResultSnapshot) {
		if (snapshotOverrideSourceKey !== incomingSnapshotKey) snapshotOverrides = {};
		snapshotOverrideSourceKey = incomingSnapshotKey;
		snapshotOverrides = {
			...snapshotOverrides,
			[nextSnapshot.match.id]: nextSnapshot
		};
	}

	function mutationAllowed(snapshot: AuctionMatchGameResultSnapshot): boolean {
		return (
			snapshot.permissions.canManageLineup &&
			!snapshot.match.lineupsLocked &&
			snapshot.match.status === 'LINEUP_PENDING'
		);
	}

	function unlockAllowed(snapshot: AuctionMatchGameResultSnapshot): boolean {
		return snapshot.permissions.canUnlockLineup && snapshot.match.lineupsLocked;
	}

	async function recoverAuthoritativeSnapshot(
		matchId: string,
		conflictMessage: string
	): Promise<void> {
		try {
			applySnapshot(await fetchAuctionMatchGameResult(data.guildId, matchId));
			mutationError = `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
		} catch (cause) {
			mutationError = `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${auctionMatchLineupErrorText(cause)}`;
		}
	}

	function operationName(mutationName: LineupMutation, matchId: string): string {
		return `auction-tournament-semifinal-lineup-${mutationName}:${matchId}`;
	}

	async function handleAssign(
		matchId: string,
		command: AuctionMatchLineupAssignCommand
	): Promise<void> {
		const current = semifinalSnapshots.find((snapshot) => snapshot.match.id === matchId);
		if (!current || busy || !mutationAllowed(current)) return;
		if (command.expectedRevision !== current.match.revision) {
			await recoverAuthoritativeSnapshot(matchId, '라인업 버전이 변경되었습니다.');
			throw new Error('stale_auction_tournament_lineup_assignment');
		}

		const operation = operationName('assign', matchId);
		const idempotencyKey = idempotencyKeys.keyFor(operation, command);
		busyMatchId = matchId;
		mutation = 'assign';
		mutationError = '';
		try {
			applySnapshot(
				parseAuthoritativeSnapshot(
					await assignAuctionMatchLineup(data.guildId, matchId, command, idempotencyKey)
				)
			);
			idempotencyKeys.complete(operation);
		} catch (cause) {
			if (isAuctionMatchLineupRevisionConflict(cause)) {
				idempotencyKeys.invalidate(operation);
				await recoverAuthoritativeSnapshot(matchId, auctionMatchLineupErrorText(cause));
			} else {
				mutationError =
					cause instanceof Error && cause.message === invalidSnapshotMessage
						? invalidSnapshotMessage
						: auctionMatchLineupErrorText(cause);
			}
			throw cause;
		} finally {
			busyMatchId = null;
			mutation = null;
		}
	}

	async function handleLock(
		matchId: string,
		command: AuctionMatchLineupLockCommand
	): Promise<void> {
		const current = semifinalSnapshots.find((snapshot) => snapshot.match.id === matchId);
		if (!current || busy || !mutationAllowed(current)) return;
		if (command.expectedRevision !== current.match.revision) {
			await recoverAuthoritativeSnapshot(matchId, '라인업 버전이 변경되었습니다.');
			throw new Error('stale_auction_tournament_lineup_lock');
		}

		const operation = operationName('lock', matchId);
		const idempotencyKey = idempotencyKeys.keyFor(operation, command);
		busyMatchId = matchId;
		mutation = 'lock';
		mutationError = '';
		try {
			applySnapshot(
				parseAuthoritativeSnapshot(
					await lockAuctionMatchLineup(data.guildId, matchId, command, idempotencyKey)
				)
			);
			idempotencyKeys.complete(operation);
		} catch (cause) {
			if (isAuctionMatchLineupRevisionConflict(cause)) {
				idempotencyKeys.invalidate(operation);
				await recoverAuthoritativeSnapshot(matchId, auctionMatchLineupErrorText(cause));
			} else {
				mutationError =
					cause instanceof Error && cause.message === invalidSnapshotMessage
						? invalidSnapshotMessage
						: auctionMatchLineupErrorText(cause);
			}
			throw cause;
		} finally {
			busyMatchId = null;
			mutation = null;
		}
	}

	async function handleUnlock(
		matchId: string,
		command: AuctionMatchLineupUnlockCommand
	): Promise<void> {
		const current = semifinalSnapshots.find((snapshot) => snapshot.match.id === matchId);
		if (!current || busy || !unlockAllowed(current)) return;
		if (command.expectedRevision !== current.match.revision) {
			await recoverAuthoritativeSnapshot(matchId, '라인업 버전이 변경되었습니다.');
			throw new Error('stale_auction_tournament_lineup_unlock');
		}

		const operation = operationName('unlock', matchId);
		const idempotencyKey = idempotencyKeys.keyFor(operation, command);
		busyMatchId = matchId;
		mutation = 'unlock';
		mutationError = '';
		try {
			applySnapshot(
				parseAuthoritativeSnapshot(
					await unlockAuctionMatchLineup(data.guildId, matchId, command, idempotencyKey)
				)
			);
			idempotencyKeys.complete(operation);
		} catch (cause) {
			if (isAuctionMatchLineupRevisionConflict(cause)) {
				idempotencyKeys.invalidate(operation);
				await recoverAuthoritativeSnapshot(matchId, auctionMatchLineupErrorText(cause));
			} else {
				mutationError =
					cause instanceof Error && cause.message === invalidSnapshotMessage
						? invalidSnapshotMessage
						: auctionMatchLineupErrorText(cause);
			}
			throw cause;
		} finally {
			busyMatchId = null;
			mutation = null;
		}
	}

	function mutationLabel(): string {
		if (mutation === 'assign') return '라인업 배치 반영 중';
		if (mutation === 'lock') return '라인업 확정 중';
		return '라인업 확정 해제 중';
	}
</script>

<svelte:head>
	<title>20인 준결승 라인업 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="20인 준결승 라인업"
		description={`${tournament.title} · 토너먼트 #${tournament.displayNumber} · ${rulesLabel}`}
	>
		{#snippet actions()}
			<a
				href={resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
					guildId: data.guildId,
					id: tournament.id
				})}
				class="inline-flex h-9 shrink-0 items-center justify-center border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
			>
				브래킷
			</a>
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
			{mutationLabel()}
		</div>
	{/if}

	<div class="grid gap-5">
		{#each semifinalSnapshots as semifinal (semifinal.match.id)}
			<AuctionMatchStartingLineupInputBoard
				snapshot={semifinal}
				{busy}
				onAssign={(command) => handleAssign(semifinal.match.id, command)}
				onLock={(command) => handleLock(semifinal.match.id, command)}
				onUnlock={(command) => handleUnlock(semifinal.match.id, command)}
			/>
		{/each}
	</div>

	{#if allSemifinalLineupsLocked}
		<section
			class="grid gap-2 border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] p-3"
		>
			<p class="text-sm font-semibold text-slate-100">준결승 픽밴 입력</p>
			<div class="grid gap-2 sm:grid-cols-2">
				{#each semifinalSnapshots as semifinal (semifinal.match.id)}
					<a
						href={resolve('/g/[guildId]/auction/matches/[matchId]', {
							guildId: data.guildId,
							matchId: semifinal.match.id
						})}
						class="inline-flex h-11 items-center justify-center border border-cyan-300/30 bg-cyan-400/8 px-4 text-sm font-semibold text-cyan-100 hover:bg-cyan-400/14"
					>
						{semifinal.match.label} 픽밴 입력
					</a>
				{/each}
			</div>
		</section>
	{:else}
		<p class="text-xs font-medium text-slate-500">
			두 준결승의 20명 라인업을 모두 확정하면 픽밴 결과를 입력할 수 있습니다.
		</p>
	{/if}
</div>
