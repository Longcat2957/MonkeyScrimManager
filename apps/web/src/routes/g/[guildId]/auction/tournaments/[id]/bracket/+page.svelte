<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { LoaderCircle, RotateCcw, Undo2, X } from 'lucide-svelte';
	import {
		auctionTournamentBracketErrorText,
		executeAuctionTournamentBracketCommand,
		fetchAuctionTournamentBracket,
		isAuctionTournamentBracketRevisionConflict
	} from '$lib/api/auctionTournamentBracket';
	import {
		auctionTournamentDraftErrorText,
		executeAuctionTournamentDraftCommand,
		isAuctionTournamentDraftRevisionConflict
	} from '$lib/api/auctionTournamentDraft';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentBracketMatchSetupBoard from '$lib/components/workflow/AuctionTournamentBracketMatchSetupBoard.svelte';
	import type {
		AuctionTournamentBracketCommand,
		AuctionTournamentBracketMatch,
		AuctionTournamentBracketSnapshot
	} from '$lib/contracts/auctionTournamentBracket';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let snapshotOverride = $state<AuctionTournamentBracketSnapshot | null>(null);
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let submitting = $state(false);
	let mutationError = $state('');
	let recoveryDialogOpen = $state(false);
	let recoveryError = $state('');
	let draftRecoveryDialogOpen = $state(false);
	let draftRecoveryError = $state('');
	let revertingDraft = $state(false);
	let idempotencyKeys = createOperationIdempotencyKeys();

	const incomingSnapshotKey = $derived(
		`${data.snapshot.tournament.id}:${data.snapshot.tournament.revision}:${data.snapshot.bracket.locked}`
	);
	const snapshot = $derived(
		snapshotOverride !== null && snapshotOverrideSourceKey === incomingSnapshotKey
			? snapshotOverride
			: data.snapshot
	);
	const tournament = $derived(snapshot.tournament);
	const formatLabel = $derived(tournament.format === 'TEN_PERSON' ? '10인 경매' : '20인 경매');
	const rulesLabel = $derived(formatAuctionRules(tournament.format, tournament.rules));
	const isMutating = $derived(submitting || revertingDraft);
	const canRecoverBracket = $derived(snapshot.permissions.canRecoverBracket && !isMutating);
	const canRevertDraft = $derived(snapshot.permissions.canRevertDraft && !isMutating);

	function applySnapshot(nextSnapshot: AuctionTournamentBracketSnapshot) {
		snapshotOverrideSourceKey = incomingSnapshotKey;
		snapshotOverride = nextSnapshot;
	}

	async function recoverAuthoritativeSnapshot(conflictMessage: string, surfaceError = true) {
		try {
			const nextSnapshot = await fetchAuctionTournamentBracket(data.guildId, tournament.id);
			applySnapshot(nextSnapshot);
			if (surfaceError) {
				mutationError = `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
			}
		} catch (refreshCause) {
			if (surfaceError) {
				mutationError = `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${auctionTournamentBracketErrorText(refreshCause)}`;
			}
		}
	}

	function firstPlayableMatch(nextSnapshot: AuctionTournamentBracketSnapshot) {
		const order: Record<AuctionTournamentBracketMatch['matchKey'], number> = {
			SEMIFINAL_A: 0,
			SEMIFINAL_B: 1,
			FINAL: 2
		};
		return nextSnapshot.bracket.matches
			.slice()
			.sort((left, right) => order[left.matchKey] - order[right.matchKey])
			.find(
				(match) =>
					match.leftTeamId !== null &&
					match.rightTeamId !== null &&
					match.status !== 'COMPLETED' &&
					match.status !== 'CANCELLED'
			);
	}

	function opensTournamentLineup(
		match: AuctionTournamentBracketMatch | undefined,
		bracketSnapshot: AuctionTournamentBracketSnapshot
	): boolean {
		const semifinalMatches = bracketSnapshot.bracket.matches.filter(
			(candidate) =>
				candidate.matchKey === 'SEMIFINAL_A' || candidate.matchKey === 'SEMIFINAL_B'
		);
		return (
			bracketSnapshot.tournament.format === 'TWENTY_PERSON' &&
			match !== undefined &&
			(match.matchKey === 'SEMIFINAL_A' || match.matchKey === 'SEMIFINAL_B') &&
			semifinalMatches.length === 2 &&
			!semifinalMatches.every((candidate) => candidate.lineupsLocked)
		);
	}

	async function openMatch(
		matchId: string,
		bracketSnapshot: AuctionTournamentBracketSnapshot = snapshot
	) {
		const match = bracketSnapshot.bracket.matches.find((candidate) => candidate.id === matchId);
		if (opensTournamentLineup(match, bracketSnapshot)) {
			await goto(
				resolve('/g/[guildId]/auction/tournaments/[id]/lineup', {
					guildId: data.guildId,
					id: bracketSnapshot.tournament.id
				})
			);
			return;
		}

		await goto(
			resolve('/g/[guildId]/auction/matches/[matchId]', {
				guildId: data.guildId,
				matchId
			})
		);
	}

	async function handleBracketCommand(
		command: AuctionTournamentBracketCommand,
		surfaceError = true
	): Promise<void> {
		const allowed =
			command.kind === 'UNLOCK_BRACKET'
				? snapshot.permissions.canRecoverBracket
				: snapshot.permissions.canManageBracket;
		if (isMutating || !allowed) return;
		if (command.expectedRevision !== snapshot.tournament.revision) {
			await recoverAuthoritativeSnapshot('브래킷 버전이 변경되었습니다.', surfaceError);
			throw new Error('stale_auction_tournament_bracket_command');
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-tournament-bracket-command', command);
		const sourceSnapshotKey = incomingSnapshotKey;
		submitting = true;
		mutationError = '';
		try {
			const nextSnapshot = await executeAuctionTournamentBracketCommand(
				data.guildId,
				tournament.id,
				command,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) applySnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-tournament-bracket-command');

			if (command.kind === 'LOCK_BRACKET') {
				const nextMatch = firstPlayableMatch(nextSnapshot);
				if (nextMatch) await openMatch(nextMatch.id, nextSnapshot);
			}
		} catch (cause) {
			if (isAuctionTournamentBracketRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-bracket-command');
				await recoverAuthoritativeSnapshot(auctionTournamentBracketErrorText(cause), surfaceError);
			} else if (surfaceError) {
				mutationError = auctionTournamentBracketErrorText(cause);
			}
			throw cause;
		} finally {
			submitting = false;
		}
	}

	function openRecoveryDialog() {
		if (!canRecoverBracket) return;
		recoveryError = '';
		recoveryDialogOpen = true;
	}

	async function confirmBracketRecovery(): Promise<void> {
		if (!canRecoverBracket) return;
		recoveryError = '';
		try {
			await handleBracketCommand(
				{ kind: 'UNLOCK_BRACKET', expectedRevision: tournament.revision },
				false
			);
			recoveryDialogOpen = false;
		} catch (cause) {
			recoveryError = auctionTournamentBracketErrorText(cause);
		}
	}

	function openDraftRecoveryDialog() {
		if (!canRevertDraft) return;
		draftRecoveryError = '';
		draftRecoveryDialogOpen = true;
	}

	async function confirmDraftRecovery(): Promise<void> {
		if (!canRevertDraft) return;

		const command = { kind: 'REVERT_LAST', expectedRevision: tournament.revision } as const;
		const idempotencyKey = idempotencyKeys.keyFor(
			'auction-tournament-revert-last-draft',
			command
		);
		revertingDraft = true;
		draftRecoveryError = '';
		try {
			await executeAuctionTournamentDraftCommand(data.guildId, tournament.id, command, idempotencyKey);
			idempotencyKeys.complete('auction-tournament-revert-last-draft');
			draftRecoveryDialogOpen = false;
			await goto(
				resolve('/g/[guildId]/auction/tournaments/[id]/draft', {
					guildId: data.guildId,
					id: tournament.id
				})
			);
		} catch (cause) {
			if (isAuctionTournamentDraftRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-revert-last-draft');
				await recoverAuthoritativeSnapshot(auctionTournamentDraftErrorText(cause), false);
			}
			draftRecoveryError = auctionTournamentDraftErrorText(cause);
		} finally {
			revertingDraft = false;
		}
	}
</script>

<svelte:head>
	<title>경매 브래킷 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="토너먼트 브래킷"
		description={`${tournament.title} · 토너먼트 #${tournament.displayNumber} · ${formatLabel} · ${rulesLabel}`}
	>
		{#snippet actions()}
			{#if canRecoverBracket}
				<button
					type="button"
					onclick={openRecoveryDialog}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-amber-300/30 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/10"
				>
					<RotateCcw size={14} aria-hidden="true" /> 대진 확정 해제
				</button>
			{/if}
			{#if canRevertDraft}
				<button
					type="button"
					onclick={openDraftRecoveryDialog}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-amber-300/30 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/10"
				>
					<Undo2 size={14} aria-hidden="true" /> 입찰 단계로 되돌리기
				</button>
			{/if}
			<a
				href={resolve('/g/[guildId]/auction/tournaments/[id]/draft', {
					guildId: data.guildId,
					id: tournament.id
				})}
				class="inline-flex h-9 shrink-0 items-center justify-center border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
			>
				입찰 내역
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

	{#if isMutating}
		<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
			<LoaderCircle size={15} class="animate-spin" aria-hidden="true" />
			{revertingDraft ? '입찰 단계로 되돌리는 중' : '브래킷 반영 중'}
		</div>
	{/if}

	<AuctionTournamentBracketMatchSetupBoard
		{snapshot}
		onCommand={handleBracketCommand}
		onOpenMatch={openMatch}
		busy={isMutating}
	/>
</div>

<Dialog.Root bind:open={recoveryDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex items-center justify-between gap-3">
				<Dialog.Title class="text-sm font-semibold text-slate-50">대진 확정 해제</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="대진 확정 해제 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</div>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				현재 대진과 생성된 경기는 취소 이력으로 보존하고, 팀 배치 단계로 되돌립니다. 픽밴
				입력이나 결과가 하나라도 있으면 이 작업은 진행되지 않습니다.
			</Dialog.Description>
			{#if recoveryError}
				<p
					role="alert"
					class="mt-3 border border-rose-300/30 bg-rose-400/10 px-3 py-2 text-xs leading-5 text-rose-100"
				>
					{recoveryError}
				</p>
			{/if}
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={isMutating}
					class="h-8 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={!canRecoverBracket}
					onclick={() => void confirmBracketRecovery()}
					class="inline-flex h-8 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if submitting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<RotateCcw size={14} aria-hidden="true" /> 확정 해제
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={draftRecoveryDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex items-center justify-between gap-3">
				<Dialog.Title class="text-sm font-semibold text-slate-50">입찰 단계로 되돌리기</Dialog.Title>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="입찰 단계로 되돌리기 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</div>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				마지막 입찰 작업과 해당 선수 배치·포인트를 함께 되돌리고, 토너먼트를 입찰 단계로
				복구합니다. 아직 확정되지 않은 대진 슬롯은 비워집니다.
			</Dialog.Description>
			{#if draftRecoveryError}
				<p
					role="alert"
					class="mt-3 border border-rose-300/30 bg-rose-400/10 px-3 py-2 text-xs leading-5 text-rose-100"
				>
					{draftRecoveryError}
				</p>
			{/if}
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={isMutating}
					class="h-8 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={!canRevertDraft}
					onclick={() => void confirmDraftRecovery()}
					class="inline-flex h-8 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if revertingDraft}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<Undo2 size={14} aria-hidden="true" /> 입찰 단계로 되돌리기
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
