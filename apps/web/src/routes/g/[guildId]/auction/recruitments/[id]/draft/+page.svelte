<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { LoaderCircle, Mail } from 'lucide-svelte';
	import {
		auctionRecruitmentErrorText,
		dispatchAuctionRecruitmentParticipantConfirmationDms
	} from '$lib/api/auctionRecruitment';
	import {
		auctionTournamentErrorText,
		createAuctionTournament,
		fetchAuctionSetupDraft,
		isAuctionSetupDraftRevisionConflict,
		saveAuctionSetupDraft
	} from '$lib/api/auctionTournament';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import AuctionTournamentCaptainPointSetupBoard, {
		type AuctionSetupBoardCommand
	} from '$lib/components/workflow/AuctionTournamentCaptainPointSetupBoard.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { AuctionSetupDraftSnapshot } from '$lib/contracts/auctionTournamentSetup';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let setupOverride = $state<AuctionSetupDraftSnapshot | null>(null);
	let setupOverrideSourceKey = $state<string | null>(null);
	let saving = $state(false);
	let creating = $state(false);
	let dispatchingParticipantConfirmationDms = $state(false);
	let participantConfirmationDmIdempotencyKey = $state<string | null>(null);
	let mutationError = $state('');
	let mutationNotice = $state('');
	let idempotencyKeys = createOperationIdempotencyKeys();

	const incomingSetupKey = $derived(
		`${data.setup.draft.recruitmentId}:${data.setup.draft.revision}`
	);
	const setup = $derived(
		setupOverride !== null && setupOverrideSourceKey === incomingSetupKey
			? setupOverride
			: data.setup
	);
	const recruitment = $derived(data.snapshot.recruitment);
	const formatLabel = $derived(recruitment.format === 'TEN_PERSON' ? '10인 경매' : '20인 경매');
	const rulesLabel = $derived(formatAuctionRules(recruitment.format, recruitment.rules));
	const canSendParticipantConfirmationDms = $derived(
		data.snapshot.permissions.canSendParticipantConfirmationDms
	);
	const isMutating = $derived(saving || creating || dispatchingParticipantConfirmationDms);

	function applySetupSnapshot(nextSnapshot: AuctionSetupDraftSnapshot) {
		setupOverrideSourceKey = incomingSetupKey;
		setupOverride = nextSnapshot;
	}

	async function recoverAuthoritativeSetup(conflictMessage: string) {
		try {
			const nextSnapshot = await fetchAuctionSetupDraft(data.guildId, recruitment.id);
			applySetupSnapshot(nextSnapshot);
			mutationError = `${conflictMessage} 서버 최신 설정을 불러왔습니다. 변경 내용을 확인한 뒤 다시 시도해 주세요.`;
		} catch (refreshCause) {
			mutationError = `${conflictMessage} 최신 설정을 불러오지 못했습니다. ${auctionTournamentErrorText(refreshCause)}`;
		}
	}

	async function saveSetup(command: AuctionSetupBoardCommand) {
		if (isMutating || !setup.permissions.canEdit) return;
		if (command.expectedRevision !== setup.draft.revision) {
			await recoverAuthoritativeSetup('경매 준비 설정 버전이 변경되었습니다.');
			return;
		}

		const idempotencyKey = idempotencyKeys.keyFor('auction-setup-save', command);
		saving = true;
		mutationError = '';
		mutationNotice = '';
		try {
			const nextSnapshot = await saveAuctionSetupDraft(data.guildId, recruitment.id, {
				...command,
				idempotencyKey
			});
			applySetupSnapshot(nextSnapshot);
			idempotencyKeys.complete('auction-setup-save');
			mutationNotice = '경매 준비 설정을 저장했습니다.';
		} catch (cause) {
			if (isAuctionSetupDraftRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-setup-save');
				await recoverAuthoritativeSetup(auctionTournamentErrorText(cause));
			} else {
				mutationError = auctionTournamentErrorText(cause);
			}
		} finally {
			saving = false;
		}
	}

	async function startTournament(command: AuctionSetupBoardCommand) {
		if (isMutating || !setup.permissions.canCreateTournament) return;
		if (command.expectedRevision !== setup.draft.revision) {
			await recoverAuthoritativeSetup('경매 준비 설정 버전이 변경되었습니다.');
			return;
		}

		const operation = {
			sourceRecruitmentId: setup.draft.recruitmentId,
			expectedRevision: command.expectedRevision
		};
		const idempotencyKey = idempotencyKeys.keyFor('auction-setup-create-tournament', operation);
		creating = true;
		mutationError = '';
		mutationNotice = '';
		try {
			const tournament = await createAuctionTournament(data.guildId, {
				...operation,
				idempotencyKey
			});
			idempotencyKeys.complete('auction-setup-create-tournament');
			await goto(tournament.href);
		} catch (cause) {
			if (isAuctionSetupDraftRevisionConflict(cause)) {
				idempotencyKeys.invalidate('auction-setup-create-tournament');
				await recoverAuthoritativeSetup(auctionTournamentErrorText(cause));
			} else {
				mutationError = auctionTournamentErrorText(cause);
			}
		} finally {
			creating = false;
		}
	}

	async function sendParticipantConfirmationDms() {
		if (isMutating || !canSendParticipantConfirmationDms) return;

		const idempotencyKey = (participantConfirmationDmIdempotencyKey ??=
			globalThis.crypto.randomUUID());
		dispatchingParticipantConfirmationDms = true;
		mutationError = '';
		mutationNotice = '';
		try {
			const result = await dispatchAuctionRecruitmentParticipantConfirmationDms(
				data.guildId,
				recruitment.id,
				idempotencyKey
			);
			participantConfirmationDmIdempotencyKey = null;
			if (result.queuedRecipientCount > 0) {
				mutationNotice = `참가자 ${result.queuedRecipientCount}명에게 확정 DM 발송을 요청했습니다.`;
			} else if (result.alreadyQueuedRecipientCount > 0) {
				mutationNotice = `참가자 ${result.alreadyQueuedRecipientCount}명에게는 이미 확정 DM 발송이 요청되어 있습니다.`;
			} else {
				mutationNotice = 'DM을 받을 Discord 참가자가 없습니다.';
			}
		} catch (cause) {
			mutationError = auctionRecruitmentErrorText(cause);
		} finally {
			dispatchingParticipantConfirmationDms = false;
		}
	}
</script>

<svelte:head>
	<title>경매 준비 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="경매 준비"
		description={`${recruitment.title} · 모집 #${recruitment.displayNumber} · ${formatLabel} · ${rulesLabel}`}
	>
		{#snippet actions()}
			{#if canSendParticipantConfirmationDms}
				<button
					type="button"
					disabled={isMutating}
					onclick={sendParticipantConfirmationDms}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-violet-300/35 bg-violet-400/10 px-3 text-xs font-semibold text-violet-50 enabled:hover:bg-violet-400/20 disabled:cursor-not-allowed disabled:opacity-45"
				>
					{#if dispatchingParticipantConfirmationDms}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> DM 요청 중
					{:else}
						<Mail size={14} aria-hidden="true" /> 참가 확정 DM 발송
					{/if}
				</button>
			{/if}
			<a
				href={resolve('/g/[guildId]/auction/recruitments/[id]', {
					guildId: data.guildId,
					id: recruitment.id
				})}
				class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/30 hover:bg-cyan-400/5"
			>
				모집 상세
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
	{:else if mutationNotice}
		<div
			role="status"
			class="flex min-h-10 items-center gap-2 border border-emerald-300/25 bg-emerald-400/8 px-3 text-xs font-medium text-emerald-100"
		>
			{mutationNotice}
		</div>
	{/if}

	{#if creating}
		<div class="flex items-center gap-2 text-xs font-medium text-slate-400">
			<LoaderCircle size={15} class="animate-spin" aria-hidden="true" /> 입찰 작업대를 생성하고 있습니다.
		</div>
	{/if}

	<AuctionTournamentCaptainPointSetupBoard
		draft={setup.draft}
		participants={setup.participants}
		canEdit={setup.permissions.canEdit}
		canCreateTournament={setup.permissions.canCreateTournament}
		busy={isMutating}
		onSave={saveSetup}
		onCreate={startTournament}
	/>
</div>
