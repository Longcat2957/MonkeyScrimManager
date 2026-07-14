<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, LoaderCircle } from 'lucide-svelte';
	import {
		auctionTournamentDraftErrorText,
		executeAuctionTournamentDraftCommand,
		fetchAuctionTournamentDraft,
		isAuctionTournamentDraftRevisionConflict
	} from '$lib/api/auctionTournamentDraft';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentBidDraftWorkbench from '$lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte';
	import type {
		AuctionTournamentDraftCommand,
		AuctionTournamentDraftSnapshot
	} from '$lib/contracts/auctionTournamentDraft';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let snapshotOverride = $state<AuctionTournamentDraftSnapshot | null>(null);
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let submitting = $state(false);
	let mutationError = $state('');
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

	function applySnapshot(nextSnapshot: AuctionTournamentDraftSnapshot) {
		snapshotOverrideSourceKey = incomingSnapshotKey;
		snapshotOverride = nextSnapshot;
	}

	async function recoverAuthoritativeSnapshot(conflictMessage: string) {
		try {
			const nextSnapshot = await fetchAuctionTournamentDraft(data.guildId, tournament.id);
			applySnapshot(nextSnapshot);
			mutationError = `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
		} catch (refreshCause) {
			mutationError = `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${auctionTournamentDraftErrorText(refreshCause)}`;
		}
	}

	async function handleDraftCommand(command: AuctionTournamentDraftCommand): Promise<void> {
		if (submitting || !snapshot.permissions.canManageDraft) return;
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
			<a
				href={resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
					guildId: data.guildId,
					id: tournament.id
				})}
				class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
			>
				브래킷 <ArrowRight size={14} aria-hidden="true" />
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

	{#if submitting}
		<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
			<LoaderCircle size={15} class="animate-spin" aria-hidden="true" /> 경매 드래프트 반영 중
		</div>
	{/if}

	<AuctionTournamentBidDraftWorkbench {snapshot} onCommand={handleDraftCommand} {submitting} />
</div>
