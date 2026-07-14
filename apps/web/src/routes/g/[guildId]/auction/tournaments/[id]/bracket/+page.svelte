<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { LoaderCircle } from 'lucide-svelte';
	import {
		auctionTournamentBracketErrorText,
		executeAuctionTournamentBracketCommand,
		fetchAuctionTournamentBracket,
		isAuctionTournamentBracketRevisionConflict
	} from '$lib/api/auctionTournamentBracket';
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

	function applySnapshot(nextSnapshot: AuctionTournamentBracketSnapshot) {
		snapshotOverrideSourceKey = incomingSnapshotKey;
		snapshotOverride = nextSnapshot;
	}

	async function recoverAuthoritativeSnapshot(conflictMessage: string) {
		try {
			const nextSnapshot = await fetchAuctionTournamentBracket(data.guildId, tournament.id);
			applySnapshot(nextSnapshot);
			mutationError = `${conflictMessage} 서버 최신 상태를 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
		} catch (refreshCause) {
			mutationError = `${conflictMessage} 최신 상태를 불러오지 못했습니다. ${auctionTournamentBracketErrorText(refreshCause)}`;
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

	async function openMatch(matchId: string) {
		await goto(
			resolve('/g/[guildId]/auction/matches/[matchId]', {
				guildId: data.guildId,
				matchId
			})
		);
	}

	async function handleBracketCommand(command: AuctionTournamentBracketCommand): Promise<void> {
		if (submitting || !snapshot.permissions.canManageBracket) return;
		if (command.expectedRevision !== snapshot.tournament.revision) {
			await recoverAuthoritativeSnapshot('브래킷 버전이 변경되었습니다.');
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
				if (nextMatch) await openMatch(nextMatch.id);
			}
		} catch (cause) {
			if (isAuctionTournamentBracketRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-bracket-command');
				await recoverAuthoritativeSnapshot(auctionTournamentBracketErrorText(cause));
			} else {
				mutationError = auctionTournamentBracketErrorText(cause);
			}
			throw cause;
		} finally {
			submitting = false;
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
			{#if tournament.status === 'COMPLETED'}
				<a
					href={resolve('/g/[guildId]/auction/tournaments/[id]/result', {
						guildId: data.guildId,
						id: tournament.id
					})}
					class="inline-flex h-9 shrink-0 items-center justify-center border border-emerald-300/30 px-3 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/10"
				>
					토너먼트 결과
				</a>
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

	{#if submitting}
		<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
			<LoaderCircle size={15} class="animate-spin" aria-hidden="true" /> 브래킷 반영 중
		</div>
	{/if}

	<AuctionTournamentBracketMatchSetupBoard
		{snapshot}
		onCommand={handleBracketCommand}
		onOpenMatch={openMatch}
		busy={submitting}
	/>
</div>
