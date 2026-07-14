<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Dialog } from 'bits-ui';
	import { SvelteMap } from 'svelte/reactivity';
	import {
		ArrowRight,
		LoaderCircle,
		Plus,
		Search,
		Trash2,
		UserPlus,
		UsersRound,
		X
	} from 'lucide-svelte';
	import {
		addRecruitmentMember,
		cancelRecruitment,
		closeRecruitmentForEntry,
		fetchRecruitmentSnapshot,
		removeRecruitmentMember,
		recruitmentManagementErrorText
	} from '$lib/api/recruitmentManagement';
	import { fetchGuildAdminMembers, GuildAdminMembersApiError } from '$lib/api/guildAdminMembers';
	import type { RecruitmentSnapshot } from '$lib/contracts/recruitmentSnapshot';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { GuildAdminMembersSnapshot } from '$lib/contracts/workflow';
	import {
		createBrowserRealtimeSocket,
		type BrowserRealtimeSocket,
		type RealtimeInvalidateEvent
	} from '$lib/realtime/browserSocket';
	import {
		isNewerRecruitmentInvalidation,
		recruitmentRealtimeTopic
	} from '$lib/realtime/recruitment';
	import { formatSeriesRules } from '$lib/workflow/seriesRules';
	import type { PageProps } from './$types';

	type RecruitmentRoute = {
		key: string;
		guildId: string;
		recruitmentId: string;
	};

	let { data }: PageProps = $props();

	let snapshotOverride = $state<RecruitmentSnapshot | null>(null);
	let snapshotOverrideSourceKey = $state<string | null>(null);
	let memberSnapshot = $state<GuildAdminMembersSnapshot | null>(null);
	let memberPickerOpen = $state(false);
	let memberPickerLoading = $state(false);
	let memberPickerError = $state('');
	let candidateQuery = $state('');
	let addingMemberId = $state<string | null>(null);
	let removingMemberId = $state<string | null>(null);
	let mutationNotice = $state('');
	let removeError = $state('');
	let addIdempotencyKeys = new SvelteMap<string, string>();
	let removeIdempotencyKeys = new SvelteMap<string, string>();
	let entryStarting = $state(false);
	let entryStartError = $state('');
	let entryStartIdempotencyKey = $state<string | null>(null);
	let cancelDialogOpen = $state(false);
	let cancelSubmitting = $state(false);
	let cancelError = $state('');
	let cancelIdempotencyKey = $state<string | null>(null);
	let realtimeRefreshPendingRouteKey = $state<string | null>(null);
	let realtimeRefreshingRouteKey = $state<string | null>(null);
	let recruitmentRealtimeSocket: BrowserRealtimeSocket | null = null;

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
	const participantCount = $derived(participants.length);
	const isOpen = $derived(recruitment.status === 'OPEN');
	const isOperator = $derived(data.guild.currentUserRole === 'OPERATOR');
	const canCancel = $derived(isOperator || data.user.discordId === recruitment.createdById);
	const isAtCapacity = $derived(participantCount >= recruitment.targetCount);
	const isRosterMutating = $derived(addingMemberId !== null || removingMemberId !== null);
	const isRecruitmentMutating = $derived(isRosterMutating || entryStarting || cancelSubmitting);
	const canStartEntry = $derived(
		isOpen && isOperator && participantCount === recruitment.targetCount
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

	$effect(() => {
		if (!isOperator) return;

		const route = currentRecruitmentRoute();
		const topic = recruitmentRealtimeTopic(route.guildId, route.recruitmentId);
		const client = createBrowserRealtimeSocket({
			topics: [topic],
			onConnectionStateChange: (state) => {
				if (state === 'connected') queueRealtimeSnapshotRefresh(route);
			},
			onInvalidate: (event) => handleRealtimeInvalidation(event, route, topic),
			onResyncRequired: () => queueRealtimeSnapshotRefresh(route)
		});

		recruitmentRealtimeSocket = client;
		client.connect();
		return () => {
			if (recruitmentRealtimeSocket === client) recruitmentRealtimeSocket = null;
			client.disconnect();
		};
	});

	const laneLabels = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '바텀',
		SUPPORT: '서포트'
	} as const;

	function participantInitial(displayName: string): string {
		return displayName.slice(0, 1).toLocaleUpperCase('ko-KR');
	}

	function preferredLanesLabel(lanes: Array<keyof typeof laneLabels>): string {
		return lanes.length > 0 ? lanes.map((lane) => laneLabels[lane]).join(' · ') : '미설정';
	}

	function memberDetail(mainLane: keyof typeof laneLabels | null): string {
		return mainLane ? `주 포지션 · ${laneLabels[mainLane]}` : '주 포지션 미설정';
	}

	function errorText(cause: unknown): string {
		if (cause instanceof GuildAdminMembersApiError) {
			return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
		}
		return recruitmentManagementErrorText(cause);
	}

	function currentRecruitmentRoute(): RecruitmentRoute {
		return {
			key: `${data.guildId}:${data.snapshot.recruitment.id}`,
			guildId: data.guildId,
			recruitmentId: data.snapshot.recruitment.id
		};
	}

	function isCurrentRecruitmentRoute(route: RecruitmentRoute): boolean {
		return currentRecruitmentRoute().key === route.key;
	}

	function handleRealtimeInvalidation(
		event: RealtimeInvalidateEvent,
		route: RecruitmentRoute,
		topic: string
	) {
		if (!isCurrentRecruitmentRoute(route)) return;
		if (!isNewerRecruitmentInvalidation(event, snapshot, topic)) return;
		queueRealtimeSnapshotRefresh(route);
	}

	function queueRealtimeSnapshotRefresh(route: RecruitmentRoute) {
		if (!isCurrentRecruitmentRoute(route)) return;
		realtimeRefreshPendingRouteKey = route.key;
		void flushRealtimeSnapshotRefresh(route);
	}

	async function flushRealtimeSnapshotRefresh(route: RecruitmentRoute) {
		if (!isCurrentRecruitmentRoute(route)) return;
		if (realtimeRefreshPendingRouteKey !== route.key) return;
		if (realtimeRefreshingRouteKey === route.key || isRecruitmentMutating) return;

		realtimeRefreshPendingRouteKey = null;
		realtimeRefreshingRouteKey = route.key;
		try {
			const nextSnapshot = await fetchRecruitmentSnapshot(route.guildId, route.recruitmentId);
			if (!isCurrentRecruitmentRoute(route)) return;
			if (nextSnapshot.recruitment.revision <= snapshot.recruitment.revision) return;
			snapshotOverrideSourceKey = incomingSnapshotKey;
			snapshotOverride = nextSnapshot;
		} catch {
			// A later websocket reconnect or invalidate will retry the authoritative snapshot.
		} finally {
			if (realtimeRefreshingRouteKey === route.key) realtimeRefreshingRouteKey = null;
			if (isCurrentRecruitmentRoute(route) && realtimeRefreshPendingRouteKey === route.key) {
				void flushRealtimeSnapshotRefresh(route);
			}
		}
	}

	async function openMemberPicker() {
		if (!isOpen || !isOperator || isAtCapacity || isRecruitmentMutating) return;
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

	async function addMember(memberId: string, displayName: string) {
		if (!isOpen || !isOperator || isAtCapacity || isRecruitmentMutating) return;
		const idempotencyKey = addIdempotencyKeys.get(memberId) ?? globalThis.crypto.randomUUID();
		addIdempotencyKeys.set(memberId, idempotencyKey);
		addingMemberId = memberId;
		memberPickerError = '';
		mutationNotice = '';
		removeError = '';
		entryStartError = '';
		const sourceSnapshotKey = incomingSnapshotKey;
		try {
			const nextSnapshot = await addRecruitmentMember(
				data.guildId,
				recruitment.id,
				memberId,
				idempotencyKey
			);
			if (incomingSnapshotKey === sourceSnapshotKey) {
				snapshotOverrideSourceKey = sourceSnapshotKey;
				snapshotOverride = nextSnapshot;
			}
			addIdempotencyKeys.delete(memberId);
			mutationNotice = `${displayName}님을 참가자로 추가했습니다.`;
		} catch (cause) {
			memberPickerError = errorText(cause);
		} finally {
			addingMemberId = null;
			void flushRealtimeSnapshotRefresh(currentRecruitmentRoute());
		}
	}

	async function removeMember(memberId: string, displayName: string) {
		if (!isOpen || !isOperator || isRecruitmentMutating) return;
		const idempotencyKey = removeIdempotencyKeys.get(memberId) ?? globalThis.crypto.randomUUID();
		removeIdempotencyKeys.set(memberId, idempotencyKey);
		removingMemberId = memberId;
		mutationNotice = '';
		removeError = '';
		entryStartError = '';
		const sourceSnapshotKey = incomingSnapshotKey;
		try {
			const nextSnapshot = await removeRecruitmentMember(
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
			removeError = errorText(cause);
		} finally {
			removingMemberId = null;
			void flushRealtimeSnapshotRefresh(currentRecruitmentRoute());
		}
	}

	async function startEntryEditing() {
		if (!canStartEntry || isRecruitmentMutating) return;
		const idempotencyKey = (entryStartIdempotencyKey ??= globalThis.crypto.randomUUID());
		entryStarting = true;
		entryStartError = '';
		try {
			const nextSnapshot = await closeRecruitmentForEntry(
				data.guildId,
				recruitment.id,
				idempotencyKey
			);
			await goto(nextSnapshot.links.entry);
		} catch (cause) {
			entryStartError = errorText(cause);
		} finally {
			entryStarting = false;
			void flushRealtimeSnapshotRefresh(currentRecruitmentRoute());
		}
	}

	function openCancelDialog() {
		if (!isOpen || !canCancel || isRecruitmentMutating) return;
		cancelError = '';
		cancelDialogOpen = true;
	}

	async function confirmCancellation() {
		if (!isOpen || !canCancel || isRecruitmentMutating) return;
		const idempotencyKey = (cancelIdempotencyKey ??= globalThis.crypto.randomUUID());
		cancelSubmitting = true;
		cancelError = '';
		try {
			await cancelRecruitment(data.guildId, recruitment.id, idempotencyKey);
			await goto(resolve('/g/[guildId]/series', { guildId: data.guildId }));
		} catch (cause) {
			cancelError = errorText(cause);
		} finally {
			cancelSubmitting = false;
			void flushRealtimeSnapshotRefresh(currentRecruitmentRoute());
		}
	}
</script>

<svelte:head>
	<title>엔트리 모집 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-5">
	<WorkspaceHeader
		kicker="일반 내전"
		title="엔트리 모집"
		description={`${recruitment.title} · 모집 #${recruitment.displayNumber} · ${formatSeriesRules(recruitment.bestOf, recruitment.hardFearless)}`}
	>
		{#snippet actions()}
			{#if !isOpen}
				<a
					href={snapshot.links.entry}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
				>
					엔트리 편성 열기 <ArrowRight size={14} aria-hidden="true" />
				</a>
			{:else}
				{#if canStartEntry}
					<button
						type="button"
						disabled={isRecruitmentMutating}
						onclick={startEntryEditing}
						class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-emerald-300/35 bg-emerald-400/10 px-3 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/15 disabled:opacity-35"
					>
						{#if entryStarting}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 엔트리 편성 시작 중
						{:else}
							엔트리 편성 시작 <ArrowRight size={14} aria-hidden="true" />
						{/if}
					</button>
				{/if}
				{#if isOperator}
					<button
						type="button"
						disabled={isAtCapacity || isRecruitmentMutating}
						onclick={openMemberPicker}
						class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35"
					>
						<UserPlus size={14} aria-hidden="true" /> 참가자 추가
					</button>
				{/if}
				{#if canCancel}
					<button
						type="button"
						disabled={isRecruitmentMutating}
						onclick={openCancelDialog}
						class="grid size-9 shrink-0 place-items-center border border-rose-300/25 text-rose-200 hover:bg-rose-400/10 disabled:opacity-35"
						aria-label="모집 삭제"
						title="모집 삭제"
					>
						<Trash2 size={14} aria-hidden="true" />
					</button>
				{/if}
			{/if}
		{/snippet}
	</WorkspaceHeader>

	{#if mutationNotice}
		<p
			class="border border-emerald-400/20 bg-emerald-400/5 px-3 py-2 text-xs leading-5 text-emerald-200"
			role="status"
		>
			{mutationNotice}
		</p>
	{/if}
	{#if entryStartError}
		<p
			class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
			role="alert"
		>
			{entryStartError}
		</p>
	{/if}
	{#if removeError}
		<p
			class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
			role="alert"
		>
			{removeError}
		</p>
	{/if}

	<section
		class="flex min-h-0 flex-1 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<UsersRound size={16} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">참가자</h2>
			</div>
			<span class="text-xs font-semibold tabular-nums text-slate-500">{participantCount}명</span>
		</header>

		<div
			class="min-h-0 flex-1 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto overscroll-contain"
		>
			{#each participants as participant, index (participant.id)}
				<div
					class="grid min-h-14 grid-cols-[2rem_minmax(0,1fr)_auto_auto] items-center gap-3 px-4 py-2.5"
				>
					<span
						class="grid size-8 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-xs font-semibold text-slate-300"
					>
						{participantInitial(participant.displayName)}
					</span>
					<div class="min-w-0">
						<strong class="block truncate text-sm text-slate-100">{participant.displayName}</strong>
						<small class="block truncate text-[11px] text-slate-500">
							선호 라인 · {preferredLanesLabel(participant.preferredLanes)}
						</small>
					</div>
					<span class="text-xs tabular-nums text-slate-600">{index + 1}</span>
					{#if isOpen && isOperator}
						<button
							type="button"
							disabled={isRecruitmentMutating}
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
					아직 Discord 참가자가 없습니다.
				</div>
			{/each}
		</div>
	</section>
</div>

<Dialog.Root bind:open={memberPickerOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.5rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<div class="min-w-0">
					<Dialog.Title class="text-sm font-semibold text-slate-50">참가자 추가</Dialog.Title>
					<p class="mt-0.5 text-[10px] text-slate-600">활성 길드 멤버만 추가할 수 있습니다.</p>
				</div>
				<Dialog.Close
					disabled={isRecruitmentMutating}
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
								<div class="flex min-w-0 items-center gap-3 px-3 py-2.5">
									<span
										class="grid size-8 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-xs font-semibold text-slate-300"
									>
										{participantInitial(member.discordName)}
									</span>
									<span class="min-w-0 flex-1">
										<strong class="block truncate text-sm text-slate-100"
											>{member.discordName}</strong
										>
										<small class="block truncate text-[11px] text-slate-500">
											{memberDetail(member.mainLane)}
										</small>
									</span>
									<button
										type="button"
										disabled={isRecruitmentMutating || isAtCapacity}
										onclick={() => addMember(member.id, member.discordName)}
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
					disabled={isRecruitmentMutating}
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
				<Dialog.Title class="text-sm font-semibold text-slate-50">모집 삭제</Dialog.Title>
				<Dialog.Close
					disabled={cancelSubmitting}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="모집 삭제 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			<div class="grid gap-3 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					모집 #{recruitment.displayNumber}을 삭제하면 참가자 추가와 엔트리 편성을 계속할 수
					없습니다.
				</Dialog.Description>
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
					disabled={cancelSubmitting}
					onclick={confirmCancellation}
					class="inline-flex h-9 items-center gap-1.5 border border-rose-300/35 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 disabled:opacity-35"
				>
					{#if cancelSubmitting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 삭제 중
					{:else}
						<Trash2 size={14} aria-hidden="true" /> 모집 삭제
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
