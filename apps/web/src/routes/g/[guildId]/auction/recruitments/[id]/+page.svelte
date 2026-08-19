<script lang="ts">
	import { goto } from '$app/navigation';
	import { Dialog } from 'bits-ui';
	import { SvelteMap } from 'svelte/reactivity';
	import {
		ArrowRight,
		Gavel,
		LoaderCircle,
		Plus,
		RotateCcw,
		Search,
		Trash2,
		UserPlus,
		UsersRound,
		X
	} from 'lucide-svelte';
	import {
		addAuctionRecruitmentMember,
		auctionRecruitmentErrorText,
		cancelAuctionRecruitment,
		closeAuctionRecruitment,
		fetchAuctionRecruitment,
		isAuctionRecruitmentRevisionConflict,
		removeAuctionRecruitmentMember,
		restoreAuctionRecruitment
	} from '$lib/api/auctionRecruitment';
	import { fetchGuildAdminMembers, GuildAdminMembersApiError } from '$lib/api/guildAdminMembers';
	import type { AuctionRecruitmentSnapshot } from '$lib/contracts/auctionRecruitmentSnapshot';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { ChampionPreferenceLane, GuildAdminMembersSnapshot } from '$lib/contracts/workflow';
	import {
		createBrowserRealtimeSocket,
		type BrowserRealtimeSocket,
		type RealtimeInvalidateEvent
	} from '$lib/realtime/browserSocket';
	import {
		isNewerAuctionRecruitmentInvalidation,
		auctionRecruitmentRealtimeTopic
	} from '$lib/realtime/auctionRecruitment';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	type AuctionRecruitmentRoute = {
		key: string;
		guildId: string;
		recruitmentId: string;
	};

	let { data }: PageProps = $props();

	let snapshotOverride = $state<AuctionRecruitmentSnapshot | null>(null);
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let memberSnapshot = $state<GuildAdminMembersSnapshot | null>(null);
	let memberPickerOpen = $state(false);
	let memberPickerLoading = $state(false);
	let memberPickerError = $state('');
	let candidateQuery = $state('');
	let addingMemberId = $state<string | null>(null);
	let removingMemberId = $state<string | null>(null);
	let mutationNotice = $state('');
	let memberMutationError = $state('');
	let addIdempotencyKeys = new SvelteMap<string, string>();
	let removeIdempotencyKeys = new SvelteMap<string, string>();
	let selectedLaneByMember = new SvelteMap<string, ChampionPreferenceLane>();
	let closingRecruitment = $state(false);
	let closeError = $state('');
	let closeIdempotencyKey = $state<string | null>(null);
	let cancelDialogOpen = $state(false);
	let cancelSubmitting = $state(false);
	let cancelError = $state('');
	let cancelIdempotencyKey = $state<string | null>(null);
	let cancelConfirmationInput = $state('');
	let restoreSubmitting = $state(false);
	let restoreError = $state('');
	let restoreIdempotencyKey = $state<string | null>(null);
	let realtimeRefreshPendingRouteKey = $state<string | null>(null);
	let realtimeRefreshingRouteKey = $state<string | null>(null);
	let auctionRecruitmentRealtimeSocket: BrowserRealtimeSocket | null = null;

	const incomingSnapshotKey = $derived(
		`${data.snapshot.recruitment.id}:${data.snapshot.recruitment.revision}`
	);
	const snapshot = $derived(
		snapshotOverride !== null && snapshotOverrideSourceKey === incomingSnapshotKey
			? snapshotOverride
			: data.snapshot
	);
	const recruitment = $derived(snapshot.recruitment);
	const participants = $derived(snapshot.participants);
	const isOpen = $derived(recruitment.status === 'OPEN');
	const isCancelled = $derived(recruitment.status === 'CANCELLED');
	const canManageMembers = $derived(snapshot.permissions.canManageMembers);
	const canRestore = $derived(snapshot.permissions.canRestore);
	const cancelConfirmationText = $derived(`경매 모집 #${recruitment.displayNumber} 취소`);
	const cancelConfirmationValid = $derived(
		cancelConfirmationInput.trim() === cancelConfirmationText
	);
	const isAtCapacity = $derived(participants.length >= recruitment.participantCount);
	const canCloseRecruitment = $derived(isOpen && snapshot.permissions.canClose && isAtCapacity);
	const isRosterMutating = $derived(addingMemberId !== null || removingMemberId !== null);
	const isMutating = $derived(
		closingRecruitment || cancelSubmitting || restoreSubmitting || isRosterMutating
	);
	const participantIds = $derived(new Set(participants.map((participant) => participant.id)));
	const candidateMembers = $derived(
		(memberSnapshot?.members ?? []).filter((member) => {
			const query = candidateQuery.trim().toLocaleLowerCase();
			return (
				member.status === 'ACTIVE' &&
				!participantIds.has(member.id) &&
				(!query || member.discordName.toLocaleLowerCase().includes(query))
			);
		})
	);
	const formatLabel = $derived(recruitment.format === 'TEN_PERSON' ? '10인 경매' : '20인 경매');
	const rulesLabel = $derived(formatAuctionRules(recruitment.format, recruitment.rules));

	const laneLabels = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '바텀',
		SUPPORT: '서포트'
	} as const;
	const laneOptions = Object.keys(laneLabels) as ChampionPreferenceLane[];

	$effect(() => {
		if (!canManageMembers) return;

		const route = currentAuctionRecruitmentRoute();
		const topic = auctionRecruitmentRealtimeTopic(route.guildId, route.recruitmentId);
		const client = createBrowserRealtimeSocket({
			topics: [topic],
			onConnectionStateChange: (state) => {
				if (state === 'connected') queueRealtimeSnapshotRefresh(route);
			},
			onInvalidate: (event) => handleRealtimeInvalidation(event, route, topic),
			onResyncRequired: () => queueRealtimeSnapshotRefresh(route)
		});

		auctionRecruitmentRealtimeSocket = client;
		client.connect();
		return () => {
			if (auctionRecruitmentRealtimeSocket === client) auctionRecruitmentRealtimeSocket = null;
			client.disconnect();
		};
	});

	function participantInitial(displayName: string): string {
		return displayName.slice(0, 1).toLocaleUpperCase('ko-KR');
	}

	function declaredLaneLabel(lane: keyof typeof laneLabels | null): string {
		return lane ? laneLabels[lane] : '미설정';
	}

	function memberDetail(mainLane: ChampionPreferenceLane | null): string {
		return mainLane ? `주 포지션 · ${laneLabels[mainLane]}` : '주 포지션 미설정';
	}

	function errorText(cause: unknown): string {
		if (cause instanceof GuildAdminMembersApiError) {
			return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
		}
		return auctionRecruitmentErrorText(cause);
	}

	function currentAuctionRecruitmentRoute(): AuctionRecruitmentRoute {
		return {
			key: `${data.guildId}:${data.snapshot.recruitment.id}`,
			guildId: data.guildId,
			recruitmentId: data.snapshot.recruitment.id
		};
	}

	function isCurrentAuctionRecruitmentRoute(route: AuctionRecruitmentRoute): boolean {
		return currentAuctionRecruitmentRoute().key === route.key;
	}

	function handleRealtimeInvalidation(
		event: RealtimeInvalidateEvent,
		route: AuctionRecruitmentRoute,
		topic: string
	) {
		if (!isCurrentAuctionRecruitmentRoute(route)) return;
		if (!isNewerAuctionRecruitmentInvalidation(event, snapshot, topic)) return;
		queueRealtimeSnapshotRefresh(route);
	}

	function queueRealtimeSnapshotRefresh(route: AuctionRecruitmentRoute) {
		if (!isCurrentAuctionRecruitmentRoute(route)) return;
		realtimeRefreshPendingRouteKey = route.key;
		void flushRealtimeSnapshotRefresh(route);
	}

	async function flushRealtimeSnapshotRefresh(route: AuctionRecruitmentRoute) {
		if (!isCurrentAuctionRecruitmentRoute(route)) return;
		if (realtimeRefreshPendingRouteKey !== route.key) return;
		if (realtimeRefreshingRouteKey === route.key || isMutating) return;

		realtimeRefreshPendingRouteKey = null;
		realtimeRefreshingRouteKey = route.key;
		try {
			const nextSnapshot = await fetchAuctionRecruitment(route.guildId, route.recruitmentId);
			if (!isCurrentAuctionRecruitmentRoute(route)) return;
			if (nextSnapshot.recruitment.revision <= snapshot.recruitment.revision) return;
			snapshotOverrideSourceKey = incomingSnapshotKey;
			snapshotOverride = nextSnapshot;
		} catch {
			// A later websocket reconnect or invalidate will retry the authoritative snapshot.
		} finally {
			if (realtimeRefreshingRouteKey === route.key) realtimeRefreshingRouteKey = null;
			if (isCurrentAuctionRecruitmentRoute(route) && realtimeRefreshPendingRouteKey === route.key) {
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	function selectedLane(member: GuildAdminMembersSnapshot['members'][number]) {
		return selectedLaneByMember.get(member.id) ?? member.mainLane ?? '';
	}

	function updateSelectedLane(memberId: string, event: globalThis.Event) {
		const value = (event.currentTarget as globalThis.HTMLSelectElement).value;
		if (laneOptions.includes(value as ChampionPreferenceLane)) {
			selectedLaneByMember.set(memberId, value as ChampionPreferenceLane);
			return;
		}
		selectedLaneByMember.delete(memberId);
	}

	async function openMemberPicker() {
		if (!canManageMembers || isAtCapacity || isMutating) return;
		memberPickerOpen = true;
		memberPickerError = '';
		candidateQuery = '';
		if (memberSnapshot || memberPickerLoading) return;

		memberPickerLoading = true;
		try {
			memberSnapshot = await fetchGuildAdminMembers(data.guildId);
		} catch (cause) {
			memberPickerError = errorText(cause);
		} finally {
			memberPickerLoading = false;
		}
	}

	async function addMember(member: GuildAdminMembersSnapshot['members'][number]) {
		if (!canManageMembers || isAtCapacity || isMutating) return;
		const declaredLane = selectedLane(member);
		if (!declaredLane) {
			memberPickerError = `${member.discordName}님의 참가 라인을 선택해 주세요.`;
			return;
		}
		const idempotencyKey = addIdempotencyKeys.get(member.id) ?? globalThis.crypto.randomUUID();
		addIdempotencyKeys.set(member.id, idempotencyKey);
		addingMemberId = member.id;
		memberPickerError = '';
		memberMutationError = '';
		mutationNotice = '';
		const sourceSnapshotKey = incomingSnapshotKey;
		try {
			const nextSnapshot = await addAuctionRecruitmentMember(
				data.guildId,
				recruitment.id,
				{ memberId: member.id, declaredLane },
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) {
				snapshotOverrideSourceKey = sourceSnapshotKey;
				snapshotOverride = nextSnapshot;
			}
			addIdempotencyKeys.delete(member.id);
			mutationNotice = `${member.discordName}님을 참가자로 추가했습니다.`;
		} catch (cause) {
			memberPickerError = errorText(cause);
		} finally {
			addingMemberId = null;
			void flushRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
		}
	}

	async function removeMember(memberId: string, displayName: string) {
		if (!canManageMembers || isMutating) return;
		const idempotencyKey = removeIdempotencyKeys.get(memberId) ?? globalThis.crypto.randomUUID();
		removeIdempotencyKeys.set(memberId, idempotencyKey);
		removingMemberId = memberId;
		memberMutationError = '';
		mutationNotice = '';
		const sourceSnapshotKey = incomingSnapshotKey;
		try {
			const nextSnapshot = await removeAuctionRecruitmentMember(
				data.guildId,
				recruitment.id,
				memberId,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) {
				snapshotOverrideSourceKey = sourceSnapshotKey;
				snapshotOverride = nextSnapshot;
			}
			removeIdempotencyKeys.delete(memberId);
			mutationNotice = `${displayName}님을 참가자에서 제거했습니다.`;
		} catch (cause) {
			memberMutationError = errorText(cause);
		} finally {
			removingMemberId = null;
			void flushRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
		}
	}

	async function closeRecruitment() {
		if (!canCloseRecruitment || isMutating) return;
		const idempotencyKey = (closeIdempotencyKey ??= globalThis.crypto.randomUUID());
		closingRecruitment = true;
		closeError = '';
		try {
			const nextSnapshot = await closeAuctionRecruitment(
				data.guildId,
				recruitment.id,
				idempotencyKey
			);
			if (!nextSnapshot.links.draft) {
				closeError =
					'경매 모집은 마감되었지만 마감 명단 경로를 확인할 수 없습니다. 새로고침 후 다시 시도해 주세요.';
				return;
			}
			closeIdempotencyKey = null;
			await goto(nextSnapshot.links.draft);
		} catch (cause) {
			closeError = auctionRecruitmentErrorText(cause);
		} finally {
			closingRecruitment = false;
			void flushRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
		}
	}

	function openCancelDialog() {
		if (!snapshot.permissions.canCancel || isMutating) return;
		cancelError = '';
		cancelConfirmationInput = '';
		cancelDialogOpen = true;
	}

	async function confirmCancellation() {
		if (!snapshot.permissions.canCancel || !cancelConfirmationValid || isMutating) return;
		const idempotencyKey = (cancelIdempotencyKey ??= globalThis.crypto.randomUUID());
		cancelSubmitting = true;
		cancelError = '';
		try {
			const sourceSnapshotKey = incomingSnapshotKey;
			const nextSnapshot = await cancelAuctionRecruitment(
				data.guildId,
				recruitment.id,
				recruitment.revision,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) {
				snapshotOverrideSourceKey = sourceSnapshotKey;
				snapshotOverride = nextSnapshot;
			}
			cancelIdempotencyKey = null;
			cancelDialogOpen = false;
			mutationNotice =
				'경매 모집을 취소했습니다. 참가자와 팀장 설정은 보존되며 아래 버튼으로 복구할 수 있습니다.';
		} catch (cause) {
			if (isAuctionRecruitmentRevisionConflict(cause)) {
				cancelIdempotencyKey = null;
				queueRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
			}
			cancelError = auctionRecruitmentErrorText(cause);
		} finally {
			cancelSubmitting = false;
			void flushRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
		}
	}

	async function restoreCancellation() {
		if (!canRestore || isMutating) return;
		const idempotencyKey = (restoreIdempotencyKey ??= globalThis.crypto.randomUUID());
		restoreSubmitting = true;
		restoreError = '';
		mutationNotice = '';
		try {
			const sourceSnapshotKey = incomingSnapshotKey;
			const nextSnapshot = await restoreAuctionRecruitment(
				data.guildId,
				recruitment.id,
				recruitment.revision,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) {
				snapshotOverrideSourceKey = sourceSnapshotKey;
				snapshotOverride = nextSnapshot;
			}
			restoreIdempotencyKey = null;
			mutationNotice =
				nextSnapshot.recruitment.status === 'CLOSED'
					? '취소를 되돌리고 팀장 선택 단계로 복구했습니다.'
					: '취소를 되돌리고 모집 단계로 복구했습니다.';
		} catch (cause) {
			if (isAuctionRecruitmentRevisionConflict(cause)) {
				restoreIdempotencyKey = null;
				queueRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
			}
			restoreError = auctionRecruitmentErrorText(cause);
		} finally {
			restoreSubmitting = false;
			void flushRealtimeSnapshotRefresh(currentAuctionRecruitmentRoute());
		}
	}
</script>

<svelte:head>
	<title>경매 모집 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="경매 모집"
		description={`${recruitment.title} · 모집 #${recruitment.displayNumber} · ${formatLabel} · ${rulesLabel}`}
	>
		{#snippet actions()}
			{#if isCancelled && canRestore}
				<button
					type="button"
					disabled={isMutating}
					onclick={() => void restoreCancellation()}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/15 disabled:opacity-35"
				>
					{#if restoreSubmitting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 복구 중
					{:else}
						<RotateCcw size={14} aria-hidden="true" /> 취소 되돌리기
					{/if}
				</button>
			{/if}
			{#if canManageMembers}
				<button
					type="button"
					disabled={isAtCapacity || isMutating}
					onclick={openMemberPicker}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35"
				>
					<UserPlus size={14} aria-hidden="true" /> 참가자 추가
				</button>
			{/if}
			{#if canCloseRecruitment}
				<button
					type="button"
					disabled={isMutating}
					onclick={closeRecruitment}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-emerald-300/35 bg-emerald-400/10 px-3 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/15 disabled:opacity-35"
				>
					{#if closingRecruitment}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 모집 마감 중
					{:else}
						모집 마감 <ArrowRight size={14} aria-hidden="true" />
					{/if}
				</button>
			{/if}
			{#if !isOpen && snapshot.links.draft}
				<a
					href={snapshot.links.draft}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
				>
					마감 명단 보기 <ArrowRight size={14} aria-hidden="true" />
				</a>
			{/if}
			{#if snapshot.permissions.canCancel}
				<button
					type="button"
					disabled={isMutating}
					onclick={openCancelDialog}
					class="grid size-9 shrink-0 place-items-center border border-rose-300/25 text-rose-200 hover:bg-rose-400/10 disabled:opacity-35"
					aria-label="경매 모집 취소"
					title="경매 모집 취소"
				>
					<Trash2 size={14} aria-hidden="true" />
				</button>
			{/if}
		{/snippet}
	</WorkspaceHeader>

	{#if isCancelled}
		<p
			class="border border-amber-300/25 bg-amber-400/8 px-3 py-2 text-xs leading-5 text-amber-100"
			role="status"
		>
			이 경매 모집은 취소 이력으로 보존되어 있습니다. 참가자와 팀장 설정은 삭제되지 않았으며,
			운영자는 안전한 이전 단계로 복구할 수 있습니다.
		</p>
	{/if}

	{#if closeError}
		<p
			class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
			role="alert"
		>
			{closeError}
		</p>
	{/if}
	{#if mutationNotice}
		<p
			class="border border-emerald-400/20 bg-emerald-400/5 px-3 py-2 text-xs leading-5 text-emerald-200"
			role="status"
		>
			{mutationNotice}
		</p>
	{/if}
	{#if memberMutationError}
		<p
			class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
			role="alert"
		>
			{memberMutationError}
		</p>
	{/if}
	{#if restoreError}
		<p
			class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
			role="alert"
		>
			{restoreError}
		</p>
	{/if}

	{#if isOpen}
		<p class="text-xs leading-5 text-slate-500">
			참가와 취소는 Discord 모집 메시지에서 진행합니다. 운영자는 길드 멤버를 직접 추가하거나 제거할
			수 있으며, 모집이 마감되면 확정 명단을 확인할 수 있습니다.
		</p>
	{/if}

	<section
		class="flex min-h-0 flex-1 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<Gavel size={16} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">참가자</h2>
			</div>
			<span class="text-xs font-semibold tabular-nums text-slate-500">
				{participants.length}/{recruitment.participantCount}명
			</span>
		</header>

		<div
			class="min-h-0 flex-1 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto overscroll-contain"
		>
			{#each participants as participant, index (participant.id)}
				<div
					class={`grid min-h-14 items-center gap-3 px-4 py-2.5 ${
						canManageMembers
							? 'grid-cols-[2rem_minmax(0,1fr)_auto_2rem]'
							: 'grid-cols-[2rem_minmax(0,1fr)_auto]'
					}`}
				>
					<span
						class="grid size-8 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-xs font-semibold text-slate-300"
					>
						{participantInitial(participant.displayName)}
					</span>
					<div class="min-w-0">
						<strong class="block truncate text-sm text-slate-100">{participant.displayName}</strong>
						<small class="block truncate text-[11px] text-slate-500">
							신청 라인 · {declaredLaneLabel(participant.declaredLane)}
						</small>
					</div>
					<span class="text-xs tabular-nums text-slate-600">{index + 1}</span>
					{#if canManageMembers}
						<button
							type="button"
							disabled={isMutating}
							onclick={() => removeMember(participant.id, participant.displayName)}
							class="grid size-8 shrink-0 place-items-center border border-rose-300/25 text-rose-200 hover:bg-rose-400/10 disabled:opacity-35"
							aria-label={`${participant.displayName} 참가자에서 제거`}
							title={`${participant.displayName} 참가자에서 제거`}
						>
							{#if removingMemberId === participant.id}
								<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
							{:else}
								<Trash2 size={14} aria-hidden="true" />
							{/if}
						</button>
					{/if}
				</div>
			{:else}
				<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
					<UsersRound size={18} aria-hidden="true" />
					<p class="mt-2 text-center">
						{canManageMembers
							? 'Discord에서 참가하거나 운영자가 길드 멤버를 추가할 수 있습니다.'
							: 'Discord에서 첫 참가자를 기다리고 있습니다.'}
					</p>
				</div>
			{/each}
		</div>
	</section>
</div>

<Dialog.Root bind:open={memberPickerOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.5rem)] max-w-xl -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<div class="min-w-0">
					<Dialog.Title class="text-sm font-semibold text-slate-50">참가자 추가</Dialog.Title>
					<p class="mt-0.5 text-[10px] text-slate-600">
						활성 길드 멤버와 참가 라인을 선택해 추가합니다.
					</p>
				</div>
				<Dialog.Close
					disabled={isMutating}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="참가자 추가 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>

			<div class="min-h-0 flex-1 overflow-y-auto p-4">
				{#if memberPickerLoading}
					<div class="flex min-h-32 items-center justify-center gap-2 text-xs text-slate-500">
						<LoaderCircle size={16} class="animate-spin text-cyan-200" aria-hidden="true" />
						길드 멤버 불러오는 중
					</div>
				{:else}
					<label
						class="flex h-10 min-w-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3"
					>
						<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
						<input
							bind:value={candidateQuery}
							placeholder="길드 멤버 검색"
							aria-label="추가할 길드 멤버 검색"
							autocomplete="off"
							class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
						/>
					</label>

					{#if memberPickerError}
						<p
							class="mt-3 border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
							role="alert"
						>
							{memberPickerError}
						</p>
					{/if}

					<div class="mt-3 overflow-hidden border border-[var(--color-app-border-muted)]">
						<div class="max-h-72 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto">
							{#each candidateMembers as member (member.id)}
								<div
									class="grid min-w-0 grid-cols-[2rem_minmax(0,1fr)_5.75rem_2rem] items-center gap-3 px-3 py-2.5"
								>
									<span
										class="grid size-8 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-xs font-semibold text-slate-300"
									>
										{participantInitial(member.discordName)}
									</span>
									<span class="min-w-0">
										<strong class="block truncate text-sm text-slate-100"
											>{member.discordName}</strong
										>
										<small class="block truncate text-[11px] text-slate-500">
											{memberDetail(member.mainLane)}
										</small>
									</span>
									<select
										value={selectedLane(member)}
										onchange={(event) => updateSelectedLane(member.id, event)}
										class="h-8 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2 text-xs text-slate-200 outline-none"
										aria-label={`${member.discordName} 참가 라인`}
									>
										<option value="">라인</option>
										{#each laneOptions as lane (lane)}
											<option value={lane}>{laneLabels[lane]}</option>
										{/each}
									</select>
									<button
										type="button"
										disabled={isMutating || isAtCapacity || !selectedLane(member)}
										onclick={() => addMember(member)}
										class="grid size-8 shrink-0 place-items-center border border-cyan-400/30 text-cyan-100 hover:bg-cyan-400/10 disabled:opacity-35"
										aria-label={`${member.discordName} 참가자로 추가`}
										title={`${member.discordName} 참가자로 추가`}
									>
										{#if addingMemberId === member.id}
											<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
										{:else}
											<Plus size={14} aria-hidden="true" />
										{/if}
									</button>
								</div>
							{:else}
								<div
									class="grid min-h-28 place-items-center px-4 py-6 text-center text-sm text-slate-600"
								>
									{isAtCapacity ? '모집 정원이 찼습니다.' : '추가할 활성 길드 멤버가 없습니다.'}
								</div>
							{/each}
						</div>
					</div>
				{/if}
			</div>

			<footer class="flex justify-end border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={isMutating}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					닫기
				</Dialog.Close>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={cancelDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">경매 모집 취소</Dialog.Title>
				<Dialog.Close
					disabled={cancelSubmitting}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="경매 모집 취소 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					모집 #{recruitment.displayNumber}을 취소 상태로 전환하고 Discord 참가 메시지를 내립니다.
					참가자와 팀장 설정은 보존되며, 취소 후 이 화면에서 복구할 수 있습니다.
				</Dialog.Description>
				<label class="grid gap-1.5 text-xs font-semibold text-slate-300">
					<span>확인 문구</span>
					<input
						bind:value={cancelConfirmationInput}
						disabled={cancelSubmitting}
						placeholder={cancelConfirmationText}
						class="h-9 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-medium text-slate-100 outline-none focus:border-rose-300/50 disabled:opacity-35"
					/>
				</label>
				{#if cancelError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{cancelError}
					</p>
				{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={cancelSubmitting}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={cancelSubmitting || !cancelConfirmationValid}
					onclick={confirmCancellation}
					class="inline-flex h-9 items-center gap-1.5 border border-rose-300/35 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 disabled:opacity-35"
				>
					{#if cancelSubmitting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 취소 중
					{:else}
						<Trash2 size={14} aria-hidden="true" /> 경매 모집 취소
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
