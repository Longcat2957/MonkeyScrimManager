<script lang="ts">
	import { onMount } from 'svelte';
	import { ChevronDown, FlaskConical, LoaderCircle, UsersRound } from 'lucide-svelte';
	import {
		applyDummyRecruitmentParticipants,
		fetchDummyRecruitmentParticipants,
		previewDummyRecruitmentParticipants
	} from '$lib/api/dummyRecruitment';
	import { fetchDummyUsers } from '$lib/api/dummyUsers';
	import { devToolsErrorText } from '$lib/api/devTools';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import type {
		DummyRecruitmentAction,
		DummyRecruitmentOverview,
		DummyRecruitmentPreview
	} from '$lib/contracts/dummyRecruitment';
	import type { DummyUser } from '$lib/contracts/dummyUsers';
	import DummyMemberPicker from './DummyMemberPicker.svelte';

	interface Props {
		guildId: string;
		recruitmentId: string;
	}

	let { guildId, recruitmentId }: Props = $props();
	let expanded = $state(false);
	let loading = $state(true);
	let loadingMoreUsers = $state(false);
	let submitting = $state(false);
	let overview = $state<DummyRecruitmentOverview | null>(null);
	let users = $state<DummyUser[]>([]);
	let userTotal = $state(0);
	let action = $state<DummyRecruitmentAction>('ADD');
	let selectedIds = $state<string[]>([]);
	let preview = $state<DummyRecruitmentPreview | null>(null);
	let notice = $state('');
	let errorMessage = $state('');
	const mutationKeys = createOperationIdempotencyKeys();
	const participantOperation = 'recruitment-participants';

	const candidateById = $derived(
		new Map(overview?.candidates.map((entry) => [entry.userId, entry]) ?? [])
	);
	const eligibleUsers = $derived(
		users.filter((user) => {
			const candidate = candidateById.get(user.userId);
			if (!candidate) return false;
			return action === 'ADD'
				? candidate.participationState !== 'JOINED'
				: candidate.participationState === 'JOINED';
		})
	);

	onMount(() => {
		void load();
	});

	async function load() {
		loading = true;
		errorMessage = '';
		try {
			const [nextOverview, userPage] = await Promise.all([
				fetchDummyRecruitmentParticipants(guildId, recruitmentId),
				fetchDummyUsers({ state: 'ACTIVE', guildId, limit: 100, offset: 0 })
			]);
			overview = nextOverview;
			users = userPage.users;
			userTotal = userPage.total;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			loading = false;
		}
	}

	function setAction(next: DummyRecruitmentAction) {
		action = next;
		selectedIds = [];
		preview = null;
		mutationKeys.invalidate(participantOperation);
		notice = '';
		errorMessage = '';
	}

	function setSelection(ids: string[]) {
		selectedIds = ids;
		preview = null;
		mutationKeys.invalidate(participantOperation);
		notice = '';
	}

	async function loadMoreUsers() {
		if (loadingMoreUsers || users.length >= userTotal) return;
		loadingMoreUsers = true;
		errorMessage = '';
		try {
			const page = await fetchDummyUsers({
				state: 'ACTIVE',
				guildId,
				limit: 100,
				offset: users.length
			});
			const known = new Set(users.map((user) => user.userId));
			users = [...users, ...page.users.filter((user) => !known.has(user.userId))];
			userTotal = page.total;
		} catch (cause) {
			errorMessage = `더미 사용자 목록을 추가로 불러오지 못했습니다. ${devToolsErrorText(cause)}`;
		} finally {
			loadingMoreUsers = false;
		}
	}

	async function buildPreview() {
		if (selectedIds.length === 0 || submitting) return;
		submitting = true;
		errorMessage = '';
		notice = '';
		try {
			preview = await previewDummyRecruitmentParticipants(
				guildId,
				recruitmentId,
				action,
				selectedIds
			);
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function apply() {
		if (!preview?.canApply || submitting) return;
		const command = {
			guildId,
			recruitmentId,
			action,
			userIds: selectedIds,
			expectedRevision: preview.currentRevision
		};
		const idempotencyKey = mutationKeys.keyFor(participantOperation, command);
		submitting = true;
		errorMessage = '';
		notice = '';
		try {
			const result = await applyDummyRecruitmentParticipants(
				guildId,
				recruitmentId,
				action,
				selectedIds,
				preview.currentRevision,
				idempotencyKey
			);
			mutationKeys.complete(participantOperation);
			overview = result.overview;
			notice = `${result.changedUserIds.length}명의 모집 참가 상태를 변경했습니다.`;
			selectedIds = [];
			preview = null;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}
</script>

<section class="overflow-hidden border border-amber-300/20 bg-[var(--color-app-surface)]">
	<button
		type="button"
		onclick={() => (expanded = !expanded)}
		aria-expanded={expanded}
		class="grid min-h-12 w-full grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-3 px-4 text-left hover:bg-[var(--color-app-hover)]"
	>
		<FlaskConical size={16} class="text-amber-200" aria-hidden="true" />
		<span class="min-w-0"
			><strong class="block truncate text-xs text-slate-200">더미 참가자</strong><small
				class="block text-[10px] text-slate-600">개발 전용</small
			></span
		>
		{#if overview}<span class="text-xs font-semibold text-slate-400"
				>{overview.recruitment.joinedCount}/{overview.recruitment.targetCount}명</span
			>{/if}
		<ChevronDown
			size={15}
			class={`text-slate-500 transition-transform ${expanded ? 'rotate-180' : ''}`}
		/>
	</button>

	{#if expanded}
		<div class="border-t border-[var(--color-app-border-muted)] p-3">
			{#if loading}
				<div class="flex h-20 items-center justify-center gap-2 text-xs text-slate-500">
					<LoaderCircle size={14} class="animate-spin" /> 불러오는 중
				</div>
			{:else if overview}
				<div class="mb-3 flex flex-wrap items-center justify-between gap-3">
					<div
						class="inline-flex border border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] p-1"
					>
						<button
							type="button"
							onclick={() => setAction('ADD')}
							class={`h-8 px-3 text-xs font-semibold ${action === 'ADD' ? 'bg-cyan-400/10 text-cyan-100' : 'text-slate-500'}`}
							>참가 추가</button
						>
						<button
							type="button"
							onclick={() => setAction('REMOVE')}
							class={`h-8 px-3 text-xs font-semibold ${action === 'REMOVE' ? 'bg-cyan-400/10 text-cyan-100' : 'text-slate-500'}`}
							>참가 취소</button
						>
					</div>
					<span class="text-[10px] text-slate-600"
						>남은 자리 {overview.recruitment.remainingSlots} · r{overview.recruitment
							.revision}</span
					>
				</div>

				<DummyMemberPicker
					users={eligibleUsers}
					{selectedIds}
					requiredGuildId={guildId}
					maxSelected={action === 'ADD'
						? overview.recruitment.remainingSlots
						: overview.recruitment.joinedCount}
					disabled={submitting}
					onSelectionChange={setSelection}
				/>
				{#if users.length < userTotal}
					<button
						type="button"
						disabled={loadingMoreUsers || submitting}
						onclick={loadMoreUsers}
						class="mt-2 inline-flex h-8 w-full items-center justify-center gap-2 border border-[var(--color-app-border)] text-[11px] font-semibold text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-200 disabled:opacity-35"
					>
						{#if loadingMoreUsers}<LoaderCircle size={13} class="animate-spin" />{/if}
						더미 사용자 더 불러오기 ({users.length}/{userTotal})
					</button>
				{/if}

				{#if preview}
					<div
						class={`mt-3 border px-3 py-2 text-xs ${preview.canApply ? 'border-emerald-300/20 bg-emerald-400/5 text-emerald-200' : 'border-rose-300/20 bg-rose-400/5 text-rose-200'}`}
					>
						<p class="font-semibold">
							적용 후 {preview.projectedJoinedCount}/{overview.recruitment.targetCount}명 · 남은
							자리 {preview.remainingSlots}
						</p>
						{#each preview.issues as issue (`${issue.userId}-${issue.code}`)}<p
								class="mt-1 text-[10px]"
							>
								{issue.message}
							</p>{/each}
					</div>
				{/if}
				{#if notice}<p class="mt-3 text-xs text-emerald-200" role="status">{notice}</p>{/if}
				{#if errorMessage}<p class="mt-3 text-xs text-rose-200" role="alert">{errorMessage}</p>{/if}
				<div class="mt-3 flex justify-end gap-2">
					<button
						type="button"
						disabled={selectedIds.length === 0 || submitting}
						onclick={buildPreview}
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 disabled:opacity-35"
						>미리보기</button
					>
					<button
						type="button"
						disabled={!preview?.canApply || submitting}
						onclick={apply}
						class="inline-flex h-9 items-center gap-2 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
						><UsersRound size={14} aria-hidden="true" />
						{action === 'ADD' ? '참가 적용' : '취소 적용'}</button
					>
				</div>
			{:else if errorMessage}
				<p class="text-xs text-rose-200" role="alert">{errorMessage}</p>
			{/if}
		</div>
	{/if}
</section>
