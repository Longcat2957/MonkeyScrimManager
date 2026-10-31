<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { Flag, LoaderCircle, X } from 'lucide-svelte';
	import {
		auctionTournamentFinishErrorText,
		finishAuctionTournament,
		fetchAuctionTournamentFinishPreview,
		isAuctionTournamentFinishRevisionConflict
	} from '$lib/api/auctionTournamentFinish';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import type { AuctionTournamentFinishPreview } from '$lib/contracts/auctionTournamentFinish';

	type TournamentStatus = 'BIDDING' | 'BRACKET_SETUP' | 'IN_GAME' | 'COMPLETED' | 'CANCELLED';
	type Props = {
		guildId: string;
		tournamentId: string;
		canFinish: boolean;
		busy?: boolean;
	};

	const confirmationText = '마무리';
	const defaultReason = '운영자 요청으로 경매 내전을 마무리합니다.';

	let { guildId, tournamentId, canFinish, busy = false }: Props = $props();

	let dialogOpen = $state(false);
	let previewing = $state(false);
	let finishing = $state(false);
	let finishError = $state('');
	let finishPreview = $state<AuctionTournamentFinishPreview | null>(null);
	let finishReason = $state(defaultReason);
	let confirmationInput = $state('');
	let idempotencyKeys = createOperationIdempotencyKeys();

	const reasonLength = $derived([...finishReason].length);
	const reasonValid = $derived(finishReason.trim().length > 0 && reasonLength <= 240);
	const confirmationValid = $derived(confirmationInput.trim() === confirmationText);
	const controlBusy = $derived(busy || previewing || finishing);
	const canSubmitFinish = $derived(
		Boolean(finishPreview) && canFinish && !controlBusy && reasonValid && confirmationValid
	);

	function statusLabel(status: TournamentStatus): string {
		if (status === 'BIDDING') return '입찰 진행 중';
		if (status === 'BRACKET_SETUP') return '대진 설정 중';
		if (status === 'IN_GAME') return '경기 진행 중';
		if (status === 'COMPLETED') return '완료';
		return '취소됨';
	}

	async function openFinishDialog(): Promise<void> {
		if (!canFinish || controlBusy) return;
		dialogOpen = true;
		finishError = '';
		finishPreview = null;
		finishReason = defaultReason;
		confirmationInput = '';
		previewing = true;
		try {
			finishPreview = await fetchAuctionTournamentFinishPreview(guildId, tournamentId);
		} catch (cause) {
			finishError = auctionTournamentFinishErrorText(cause);
		} finally {
			previewing = false;
		}
	}

	async function confirmFinish(): Promise<void> {
		if (!canSubmitFinish || !finishPreview) return;
		const command = {
			expectedRevision: finishPreview.tournament.revision,
			reason: finishReason.trim()
		};
		const idempotencyKey = idempotencyKeys.keyFor('auction-tournament-finish', command);
		finishing = true;
		finishError = '';
		try {
			const result = await finishAuctionTournament(guildId, tournamentId, command, idempotencyKey);
			idempotencyKeys.complete('auction-tournament-finish');
			dialogOpen = false;
			await goto(
				resolve('/g/[guildId]/auction/tournaments/[id]/result', {
					guildId,
					id: result.tournamentId
				}),
				{ invalidateAll: true }
			);
		} catch (cause) {
			finishError = auctionTournamentFinishErrorText(cause);
			if (isAuctionTournamentFinishRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-tournament-finish');
				finishPreview = null;
				confirmationInput = '';
				try {
					finishPreview = await fetchAuctionTournamentFinishPreview(guildId, tournamentId);
					finishError += ' 최신 상태를 불러왔습니다. 다시 확인해 주세요.';
				} catch (refreshCause) {
					finishError += ` ${auctionTournamentFinishErrorText(refreshCause)}`;
				}
			}
		} finally {
			finishing = false;
		}
	}
</script>

{#if canFinish}
	<button
		type="button"
		disabled={controlBusy}
		onclick={() => void openFinishDialog()}
		class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-emerald-300/35 px-3 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/10 disabled:cursor-not-allowed disabled:opacity-35"
	>
		{#if controlBusy}
			<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 마무리
		{:else}
			<Flag size={14} aria-hidden="true" /> 마무리
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
				<Dialog.Title class="text-sm font-semibold text-slate-50">경매 내전 마무리</Dialog.Title>
				<Dialog.Close
					disabled={controlBusy}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="경매 내전 마무리 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</div>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				현재까지 확정된 경기 기록과 MMR을 유지하고 내전을 완료합니다. 10인전은 현재 경기, 20인전은
				결승에서 앞선 팀이 우승합니다. 동점이거나 결승을 진행하지 않았다면 우승팀 없이 마무리합니다.
				입력 중인 미확정 세트는 기록되지 않으며 종료 후에는 결과를 수정할 수 없습니다.
			</Dialog.Description>

			<div class="mt-4 grid gap-3">
				<div
					class="border border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-3 py-2"
				>
					{#if previewing}
						<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
							마무리 정보 확인 중
						</div>
					{:else if finishPreview}
						<div class="grid gap-1 text-xs">
							<strong class="text-slate-100"
								>A-{finishPreview.tournament.displayNumber} · {finishPreview.tournament
									.title}</strong
							>
							<span class="text-slate-500"
								>현재 상태 {statusLabel(finishPreview.tournament.status)} · 버전
								{finishPreview.tournament.revision}</span
							>
						</div>
					{:else}
						<p class="text-xs text-slate-500">마무리 정보를 불러오지 못했습니다.</p>
					{/if}
				</div>

				<label class="grid gap-1.5 text-xs font-semibold text-slate-300">
					<span>마무리 사유</span>
					<textarea
						bind:value={finishReason}
						disabled={controlBusy}
						maxlength="240"
						rows="3"
						class="min-h-20 resize-none border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 py-2 text-xs font-medium text-slate-100 outline-none focus:border-emerald-300/50 disabled:opacity-35"
					></textarea>
					<span class={reasonValid ? 'text-slate-500' : 'text-rose-300'}>{reasonLength}/240</span>
				</label>

				<label class="grid gap-1.5 text-xs font-semibold text-slate-300">
					<span>확인 문구</span>
					<input
						bind:value={confirmationInput}
						disabled={controlBusy}
						placeholder={confirmationText}
						class="h-9 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-medium text-slate-100 outline-none focus:border-emerald-300/50 disabled:opacity-35"
					/>
				</label>
			</div>

			{#if finishError}
				<p
					role="alert"
					class="mt-3 border border-rose-300/30 bg-rose-400/10 px-3 py-2 text-xs leading-5 text-rose-100"
				>
					{finishError}
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
					disabled={!canSubmitFinish}
					onclick={() => void confirmFinish()}
					class="inline-flex h-8 items-center gap-1.5 border border-emerald-300/40 bg-emerald-400/10 px-3 text-xs font-semibold text-emerald-100 enabled:hover:bg-emerald-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if finishing}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 종료 중
					{:else}
						<Flag size={14} aria-hidden="true" /> 마무리
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
