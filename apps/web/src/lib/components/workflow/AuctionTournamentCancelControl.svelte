<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { Ban, LoaderCircle, X } from 'lucide-svelte';
	import {
		auctionTournamentCancelErrorText,
		cancelAuctionTournament,
		fetchAuctionTournamentCancelPreview,
		isAuctionTournamentCancelRevisionConflict
	} from '$lib/api/auctionTournamentCancel';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import type { AuctionTournamentCancelPreview } from '$lib/contracts/auctionTournamentCancel';

	type TournamentStatus = 'BIDDING' | 'BRACKET_SETUP' | 'IN_GAME' | 'COMPLETED' | 'CANCELLED';
	type CancelTournament = {
		id: string;
		displayNumber: number;
		title: string;
		status: TournamentStatus;
		revision: number;
	};
	type Props = {
		guildId: string;
		tournament: CancelTournament;
		canCancel: boolean;
		busy?: boolean;
	};

	const confirmationText = '조기 종료';
	const defaultReason = '운영자 요청으로 경매 내전을 조기 종료합니다.';

	let { guildId, tournament, canCancel, busy = false }: Props = $props();

	let dialogOpen = $state(false);
	let previewing = $state(false);
	let cancelling = $state(false);
	let cancelError = $state('');
	let cancelPreview = $state<AuctionTournamentCancelPreview | null>(null);
	let cancelReason = $state(defaultReason);
	let confirmationInput = $state('');
	let idempotencyKeys = createOperationIdempotencyKeys();

	const reasonLength = $derived([...cancelReason].length);
	const reasonValid = $derived(cancelReason.trim().length > 0 && reasonLength <= 240);
	const confirmationValid = $derived(confirmationInput.trim() === confirmationText);
	const controlBusy = $derived(busy || previewing || cancelling);
	const canSubmitCancel = $derived(
		Boolean(cancelPreview) && canCancel && !controlBusy && reasonValid && confirmationValid
	);

	function statusLabel(status: TournamentStatus): string {
		if (status === 'BIDDING') return '입찰 진행 중';
		if (status === 'BRACKET_SETUP') return '대진 설정 중';
		if (status === 'IN_GAME') return '경기 진행 중';
		if (status === 'COMPLETED') return '완료';
		return '취소됨';
	}

	async function openCancelDialog(): Promise<void> {
		if (!canCancel || controlBusy) return;
		dialogOpen = true;
		cancelError = '';
		cancelPreview = null;
		cancelReason = defaultReason;
		confirmationInput = '';
		previewing = true;
		try {
			cancelPreview = await fetchAuctionTournamentCancelPreview(guildId, tournament.id);
		} catch (cause) {
			cancelError = auctionTournamentCancelErrorText(cause);
		} finally {
			previewing = false;
		}
	}

	async function confirmCancel(): Promise<void> {
		if (!canSubmitCancel || !cancelPreview) return;
		const command = {
			expectedRevision: cancelPreview.tournament.revision,
			reason: cancelReason.trim()
		};
		const idempotencyKey = idempotencyKeys.keyFor('auction-tournament-cancel', command);
		cancelling = true;
		cancelError = '';
		try {
			await cancelAuctionTournament(guildId, tournament.id, command, idempotencyKey);
			idempotencyKeys.complete('auction-tournament-cancel');
			dialogOpen = false;
			await goto(resolve('/g/[guildId]/auction', { guildId }));
		} catch (cause) {
			if (isAuctionTournamentCancelRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-cancel');
			}
			cancelError = auctionTournamentCancelErrorText(cause);
		} finally {
			cancelling = false;
		}
	}
</script>

{#if canCancel}
	<button
		type="button"
		disabled={controlBusy}
		onclick={() => void openCancelDialog()}
		class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-rose-300/35 px-3 text-xs font-semibold text-rose-100 hover:bg-rose-400/10 disabled:cursor-not-allowed disabled:opacity-35"
	>
		{#if controlBusy}
			<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 조기 종료
		{:else}
			<Ban size={14} aria-hidden="true" /> 조기 종료
		{/if}
	</button>
{/if}

<Dialog.Root bind:open={dialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex items-center justify-between gap-3">
				<Dialog.Title class="text-sm font-semibold text-slate-50">경매 내전 조기 종료</Dialog.Title>
				<Dialog.Close
					disabled={controlBusy}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="경매 내전 조기 종료 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</div>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				진행 중인 경매 내전을 취소 상태로 전환하고, 이미 확정된 세트와 MMR 반영을 가능한 범위에서
				역순으로 되돌립니다.
			</Dialog.Description>

			<div class="mt-4 grid gap-3">
				<div class="border border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-3 py-2">
					{#if previewing}
						<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
							조기 종료 정보 확인 중
						</div>
					{:else if cancelPreview}
						<div class="grid gap-1 text-xs">
							<strong class="text-slate-100"
								>A-{cancelPreview.tournament.displayNumber} · {cancelPreview.tournament.title}</strong
							>
							<span class="text-slate-500"
								>현재 상태 {statusLabel(cancelPreview.tournament.status)} · 버전
								{cancelPreview.tournament.revision}</span
							>
						</div>
					{:else}
						<p class="text-xs text-slate-500">조기 종료 정보를 불러오지 못했습니다.</p>
					{/if}
				</div>

				<label class="grid gap-1.5 text-xs font-semibold text-slate-300">
					<span>조기 종료 사유</span>
					<textarea
						bind:value={cancelReason}
						disabled={controlBusy}
						maxlength="240"
						rows="3"
						class="min-h-20 resize-none border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 py-2 text-xs font-medium text-slate-100 outline-none focus:border-rose-300/50 disabled:opacity-35"
					></textarea>
					<span class={reasonValid ? 'text-slate-500' : 'text-rose-300'}>{reasonLength}/240</span>
				</label>

				<label class="grid gap-1.5 text-xs font-semibold text-slate-300">
					<span>확인 문구</span>
					<input
						bind:value={confirmationInput}
						disabled={controlBusy}
						placeholder={confirmationText}
						class="h-9 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-medium text-slate-100 outline-none focus:border-rose-300/50 disabled:opacity-35"
					/>
				</label>
			</div>

			{#if cancelError}
				<p
					role="alert"
					class="mt-3 border border-rose-300/30 bg-rose-400/10 px-3 py-2 text-xs leading-5 text-rose-100"
				>
					{cancelError}
				</p>
			{/if}

			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={controlBusy}
					class="h-8 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					닫기
				</Dialog.Close>
				<button
					type="button"
					disabled={!canSubmitCancel}
					onclick={() => void confirmCancel()}
					class="inline-flex h-8 items-center gap-1.5 border border-rose-300/40 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 enabled:hover:bg-rose-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if cancelling}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 종료 중
					{:else}
						<Ban size={14} aria-hidden="true" /> 조기 종료
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
