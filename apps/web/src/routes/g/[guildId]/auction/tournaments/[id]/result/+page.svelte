<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { LoaderCircle, RotateCcw, X } from 'lucide-svelte';
	import {
		auctionTournamentBracketErrorText,
		executeAuctionTournamentBracketCommand,
		isAuctionTournamentBracketRevisionConflict
	} from '$lib/api/auctionTournamentBracket';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionTournamentCancelControl from '$lib/components/workflow/AuctionTournamentCancelControl.svelte';
	import AuctionTournamentBracketResultBoard from '$lib/components/workflow/AuctionTournamentBracketResultBoard.svelte';
	import type {
		AuctionTournamentResultMatch,
		AuctionTournamentResultSnapshot
	} from '$lib/contracts/auctionTournamentResult';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let recoveryDialogOpen = $state(false);
	let recoveringBracket = $state(false);
	let recoveryError = $state('');
	let idempotencyKeys = createOperationIdempotencyKeys();

	const tournament = $derived(data.snapshot.tournament);
	const formatLabel = $derived(tournament.format === 'TEN_PERSON' ? '10인 경매' : '20인 경매');
	const rulesLabel = $derived(formatAuctionRules(tournament.format, tournament.rules));
	const statusLabel = $derived(tournamentStatusLabel(tournament.status));
	const pageTitle = $derived(tournament.status === 'COMPLETED' ? '토너먼트 결과' : '토너먼트 현황');
	const resultEditMatch = $derived(
		data.snapshot.permissions.canRevertResults ? latestRecordedMatch(data.snapshot.matches) : null
	);
	const canRecoverBracket = $derived(
		data.snapshot.permissions.canRecoverBracket && !recoveringBracket
	);

	function tournamentStatusLabel(status: typeof tournament.status): string {
		if (status === 'BIDDING') return '입찰 진행 중';
		if (status === 'BRACKET_SETUP') return '대진 설정 중';
		if (status === 'IN_GAME') return '경기 진행 중';
		if (status === 'COMPLETED') return '완료';
		return '취소됨';
	}

	function latestRecordedMatch(
		matches: AuctionTournamentResultSnapshot['matches']
	): AuctionTournamentResultMatch | null {
		const matchOrder: Record<AuctionTournamentResultMatch['matchKey'], number> = {
			SEMIFINAL_A: 0,
			SEMIFINAL_B: 1,
			FINAL: 2
		};
		return (
			matches
				.filter((match) => match.status !== 'CANCELLED' && match.games.length > 0)
				.slice()
				.sort((left, right) => matchOrder[right.matchKey] - matchOrder[left.matchKey])[0] ?? null
		);
	}

	async function openMatch(matchId: string): Promise<void> {
		await goto(
			resolve('/g/[guildId]/auction/matches/[matchId]', {
				guildId: data.guildId,
				matchId
			})
		);
	}

	function openRecoveryDialog(): void {
		if (!canRecoverBracket) return;
		recoveryError = '';
		recoveryDialogOpen = true;
	}

	async function confirmBracketRecovery(): Promise<void> {
		if (!canRecoverBracket) return;
		const command = { kind: 'UNLOCK_BRACKET', expectedRevision: tournament.revision } as const;
		const idempotencyKey = idempotencyKeys.keyFor('auction-tournament-unlock-bracket', command);

		recoveringBracket = true;
		recoveryError = '';
		let recovered = false;
		try {
			await executeAuctionTournamentBracketCommand(
				data.guildId,
				tournament.id,
				command,
				idempotencyKey
			);
			idempotencyKeys.complete('auction-tournament-unlock-bracket');
			recoveryDialogOpen = false;
			recovered = true;
		} catch (cause) {
			if (isAuctionTournamentBracketRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-unlock-bracket');
			}
			recoveryError = auctionTournamentBracketErrorText(cause);
		} finally {
			recoveringBracket = false;
		}

		if (recovered) {
			await goto(
				resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
					guildId: data.guildId,
					id: tournament.id
				})
			);
		}
	}
</script>

<svelte:head>
	<title>{pageTitle} | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title={pageTitle}
		description={`${tournament.title} · 토너먼트 #${tournament.displayNumber} · ${formatLabel} · ${rulesLabel} · ${statusLabel}`}
	>
		{#snippet actions()}
			<AuctionTournamentCancelControl
				guildId={data.guildId}
				{tournament}
				canCancel={data.snapshot.permissions.canCancel}
				busy={recoveringBracket}
			/>
			{#if canRecoverBracket}
				<button
					type="button"
					onclick={openRecoveryDialog}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-amber-300/30 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/10"
				>
					<RotateCcw size={14} aria-hidden="true" /> 대진 확정 해제
				</button>
			{/if}
			{#if resultEditMatch}
				<a
					href={resolve('/g/[guildId]/auction/matches/[matchId]', {
						guildId: data.guildId,
						matchId: resultEditMatch.id
					})}
					class="inline-flex h-9 shrink-0 items-center justify-center border border-amber-300/30 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/10"
				>
					경기 결과 수정
				</a>
			{/if}
		{/snippet}
	</WorkspaceHeader>

	<AuctionTournamentBracketResultBoard snapshot={data.snapshot} onOpenMatch={openMatch} />
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
					disabled={recoveringBracket}
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
					disabled={recoveringBracket}
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
					{#if recoveringBracket}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<RotateCcw size={14} aria-hidden="true" /> 확정 해제
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
