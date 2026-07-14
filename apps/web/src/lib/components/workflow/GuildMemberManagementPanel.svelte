<script lang="ts">
	import { Dialog } from 'bits-ui';
	import {
		Check,
		Gauge,
		Search,
		UserCheck,
		UserPlus,
		UserRound,
		UsersRound,
		UserX,
		X
	} from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import type { GuildAdminMember, GuildMemberStatus } from '$lib/contracts/workflow';
	import { LANES, laneLabel, type Lane } from '$lib/fixtures/workflow';
	import type { GuildMemberStatusCommand, GuildMmrAdjustmentCommand } from '$lib/realtime/admin';
	import { formatRelativeTime } from '$lib/workflow/presentation';

	type StatusFilter = 'ALL' | GuildMemberStatus;
	type IdentityFilter = 'ALL' | GuildAdminMember['identityKind'];

	interface Props {
		members: GuildAdminMember[];
		revision: number;
		currentUserId: string;
		onAdjustMmr: (command: GuildMmrAdjustmentCommand) => void | Promise<void>;
		onSetStatus: (command: GuildMemberStatusCommand) => void | Promise<void>;
		dummyUsersHref?: string | null;
	}

	let {
		members,
		revision,
		currentUserId,
		onAdjustMmr,
		onSetStatus,
		dummyUsersHref = null
	}: Props = $props();
	let query = $state('');
	let statusFilter = $state<StatusFilter>('ALL');
	let identityFilter = $state<IdentityFilter>('ALL');
	let adjustmentOpen = $state(false);
	let selectedMemberId = $state<string | null>(null);
	let selectedLane = $state<Lane | ''>('');
	let adjustmentDelta = $state(0);
	let adjustmentNote = $state('');
	let adjustmentIntent = $state('');
	let adjustmentIdempotencyKey = $state('');
	let notice = $state('');
	let errorMessage = $state('');
	let submitting = $state(false);
	let statusSubmittingMemberId = $state<string | null>(null);

	const selectedMember = $derived(members.find((member) => member.id === selectedMemberId) ?? null);
	const filteredMembers = $derived(
		members.filter((member) => {
			const normalizedQuery = query.trim().toLocaleLowerCase();
			const matchesQuery =
				!normalizedQuery ||
				member.discordName.toLocaleLowerCase().includes(normalizedQuery) ||
				member.riotId?.toLocaleLowerCase().includes(normalizedQuery);
			const matchesStatus = statusFilter === 'ALL' || member.status === statusFilter;
			const matchesIdentity = identityFilter === 'ALL' || member.identityKind === identityFilter;
			return matchesQuery && matchesStatus && matchesIdentity;
		})
	);
	const canAdjust = $derived(
		selectedLane !== '' &&
			Number.isFinite(adjustmentDelta) &&
			adjustmentDelta !== 0 &&
			adjustmentNote.trim().length > 0 &&
			[...adjustmentNote.trim()].length <= 500
	);
	const mutationPending = $derived(submitting || statusSubmittingMemberId !== null);

	function openAdjustment(member: GuildAdminMember) {
		selectedMemberId = member.id;
		selectedLane = member.mainLane ?? '';
		adjustmentDelta = 0;
		adjustmentNote = '';
		adjustmentIntent = '';
		adjustmentIdempotencyKey = '';
		errorMessage = '';
		adjustmentOpen = true;
	}

	async function applyAdjustment() {
		if (!selectedMember || !selectedLane || !canAdjust || submitting) return;
		const memberName = selectedMember.discordName;
		const lane = selectedLane;
		const delta = adjustmentDelta;
		const expectedRevision = revision;
		const note = adjustmentNote.trim();
		const intent = JSON.stringify([selectedMember.id, lane, delta, note, expectedRevision]);
		if (intent !== adjustmentIntent) {
			adjustmentIntent = intent;
			adjustmentIdempotencyKey = globalThis.crypto.randomUUID();
		}
		submitting = true;
		notice = '';
		errorMessage = '';
		try {
			await onAdjustMmr({
				kind: 'ADJUST_MMR',
				idempotencyKey: adjustmentIdempotencyKey,
				memberId: selectedMember.id,
				lane,
				delta,
				note,
				expectedRevision
			});
			notice = `${memberName} ${laneLabel[lane]} MMR ${signed(delta)} 반영`;
			adjustmentOpen = false;
		} catch (cause) {
			errorMessage = errorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function toggleMember(member: GuildAdminMember) {
		if (statusSubmittingMemberId) return;
		const nextStatus: GuildMemberStatus = member.status === 'ACTIVE' ? 'INACTIVE' : 'ACTIVE';
		statusSubmittingMemberId = member.id;
		notice = '';
		errorMessage = '';
		try {
			await onSetStatus({
				kind: 'SET_MEMBER_STATUS',
				memberId: member.id,
				status: nextStatus,
				expectedRevision: revision
			});
			notice = `${member.discordName} ${nextStatus === 'ACTIVE' ? '복구' : '비활성화'} 완료`;
		} catch (cause) {
			errorMessage = errorText(cause);
		} finally {
			statusSubmittingMemberId = null;
		}
	}

	function signed(value: number): string {
		return `${value > 0 ? '+' : ''}${formatMmr(value)}`;
	}

	function formatMmr(value: number): string {
		return String(Math.round(value));
	}

	function roleLabel(role: GuildAdminMember['role']): string {
		return role === 'OPERATOR' ? '운영자' : '참가자';
	}

	function cannotDeactivateSelf(member: GuildAdminMember): boolean {
		return member.id === currentUserId && member.status === 'ACTIVE';
	}

	function statusActionLabel(member: GuildAdminMember): string {
		if (cannotDeactivateSelf(member)) return '자기 자신은 비활성화할 수 없습니다';
		return member.status === 'ACTIVE' ? '멤버 비활성화' : '멤버 복구';
	}

	function lastActiveLabel(value: string | null): string {
		return value ? formatRelativeTime(value) : '-';
	}

	function errorText(cause: unknown): string {
		return cause instanceof Error ? cause.message : '요청을 처리하지 못했습니다.';
	}
</script>

<section
	class="flex h-full min-h-0 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 shrink-0 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<UsersRound size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">길드 멤버</h2>
				<p class="text-xs text-slate-500">등록 상태와 길드 MMR 관리</p>
			</div>
		</div>
		<div class="flex items-center gap-2">
			{#if notice}
				<span
					class="hidden items-center gap-1 text-xs font-semibold text-emerald-300 lg:inline-flex"
				>
					<Check size={14} aria-hidden="true" />
					{notice}
				</span>
			{/if}
			{#if dummyUsersHref}
				<a
					href={dummyUsersHref}
					class="inline-flex h-9 items-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
				>
					<UserPlus size={14} aria-hidden="true" /> 더미 회원
				</a>
			{/if}
		</div>
	</header>
	{#if errorMessage && !adjustmentOpen}
		<div
			class="shrink-0 border-b border-rose-300/15 bg-rose-400/5 px-4 py-2 text-xs text-rose-200"
			role="alert"
		>
			{errorMessage}
		</div>
	{/if}

	<div
		class="grid shrink-0 gap-2 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-3 sm:grid-cols-[minmax(0,1fr)_140px_140px]"
	>
		<label
			class="flex h-10 min-w-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3"
		>
			<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
			<input
				bind:value={query}
				placeholder="멤버 이름 또는 Riot ID"
				aria-label="길드 멤버 검색"
				class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
			/>
		</label>
		<select
			bind:value={statusFilter}
			aria-label="멤버 상태 필터"
			class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
		>
			<option value="ALL">전체 상태</option>
			<option value="ACTIVE">활성</option>
			<option value="INACTIVE">비활성</option>
		</select>
		<select
			bind:value={identityFilter}
			aria-label="멤버 유형 필터"
			class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
		>
			<option value="ALL">전체 유형</option>
			<option value="DISCORD">Discord</option>
			<option value="DUMMY">더미</option>
		</select>
	</div>

	<div
		class="min-h-0 flex-1 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto overscroll-contain pr-px lg:hidden"
	>
		{#each filteredMembers as member (member.id)}
			<div class={`grid gap-3 p-3 ${member.status === 'INACTIVE' ? 'opacity-55' : ''}`}>
				<div class="flex min-w-0 items-start justify-between gap-3">
					<div class="flex min-w-0 items-center gap-3">
						<span
							class="grid size-9 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-400"
						>
							<UserRound size={15} aria-hidden="true" />
						</span>
						<span class="min-w-0">
							<span class="flex min-w-0 items-center gap-1.5">
								<strong class="truncate text-xs text-slate-100">{member.discordName}</strong>
								{#if member.identityKind === 'DUMMY'}
									<small class="shrink-0 text-[9px] font-semibold text-cyan-300">더미</small>
								{/if}
							</span>
							<small class="block truncate text-[10px] text-slate-600">
								{member.riotId ?? 'Riot 계정 미연결'}
							</small>
						</span>
					</div>
					<StatusPill
						label={roleLabel(member.role)}
						tone={member.role === 'OPERATOR' ? 'info' : 'neutral'}
					/>
				</div>

				<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
					<div class="min-w-0 bg-[var(--color-app-surface)] px-2 py-2">
						<span class="block text-[9px] font-semibold text-slate-600">등록</span>
						<strong
							class={`mt-0.5 block truncate text-[11px] ${member.registered ? 'text-emerald-300' : 'text-amber-200'}`}
						>
							{member.registered ? '등록됨' : '미등록'}
						</strong>
					</div>
					<div class="min-w-0 bg-[var(--color-app-surface)] px-2 py-2">
						<span class="block text-[9px] font-semibold text-slate-600">주 포지션</span>
						<strong class="mt-0.5 block truncate text-[11px] text-slate-200">
							{#if member.mainLane}
								{laneLabel[member.mainLane]} · {formatMmr(member.laneMmr[member.mainLane])}
							{:else}
								미설정
							{/if}
						</strong>
					</div>
					<div class="min-w-0 bg-[var(--color-app-surface)] px-2 py-2">
						<span class="block text-[9px] font-semibold text-slate-600">최근 활동</span>
						<strong class="mt-0.5 block truncate text-[11px] text-slate-400">
							{lastActiveLabel(member.lastActiveAt)}
						</strong>
					</div>
				</div>

				<div class="flex gap-2 pr-14 lg:pr-0">
					<button
						type="button"
						disabled={mutationPending}
						onclick={() => openAdjustment(member)}
						class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 border border-[var(--color-app-border)] px-2 text-xs font-semibold text-slate-300 hover:border-cyan-400/35 hover:text-cyan-100 disabled:opacity-35"
					>
						<Gauge size={13} aria-hidden="true" /> MMR 조정
					</button>
					<button
						type="button"
						disabled={mutationPending || cannotDeactivateSelf(member)}
						onclick={() => toggleMember(member)}
						aria-label={statusActionLabel(member)}
						title={statusActionLabel(member)}
						class={`grid size-9 shrink-0 place-items-center border ${
							member.status === 'ACTIVE'
								? 'border-rose-300/25 text-rose-200 hover:bg-rose-400/10'
								: 'border-emerald-300/25 text-emerald-200 hover:bg-emerald-400/10'
						} disabled:opacity-35`}
					>
						{#if member.status === 'ACTIVE'}
							<UserX size={14} aria-hidden="true" />
						{:else}
							<UserCheck size={14} aria-hidden="true" />
						{/if}
					</button>
				</div>
			</div>
		{:else}
			<div class="px-4 py-10 text-center text-sm text-slate-600">검색 결과 없음</div>
		{/each}
	</div>

	<div class="hidden min-h-0 flex-1 overflow-auto overscroll-contain pr-px lg:block">
		<div class="min-w-[900px]">
			<div
				class="grid grid-cols-[minmax(190px,1fr)_90px_110px_150px_90px_190px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>멤버</span><span>권한</span><span>등록</span><span>주 포지션 MMR</span><span
					>최근 활동</span
				><span></span>
			</div>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each filteredMembers as member (member.id)}
					<div
						class={`grid grid-cols-[minmax(190px,1fr)_90px_110px_150px_90px_190px] items-center px-4 py-3 ${
							member.status === 'INACTIVE' ? 'opacity-55' : ''
						}`}
					>
						<div class="flex min-w-0 items-center gap-3">
							<span
								class="grid size-9 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-400"
							>
								<UserRound size={15} aria-hidden="true" />
							</span>
							<span class="min-w-0">
								<span class="flex min-w-0 items-center gap-1.5">
									<strong class="truncate text-xs text-slate-100">{member.discordName}</strong>
									{#if member.identityKind === 'DUMMY'}
										<small class="shrink-0 text-[9px] font-semibold text-cyan-300">더미</small>
									{/if}
								</span>
								<small class="block truncate text-[10px] text-slate-600">
									{member.riotId ?? 'Riot 계정 미연결'}
								</small>
							</span>
						</div>
						<div class="justify-self-start">
							<StatusPill
								label={roleLabel(member.role)}
								tone={member.role === 'OPERATOR' ? 'info' : 'neutral'}
							/>
						</div>
						<div class="justify-self-start">
							<StatusPill
								label={member.registered ? '등록됨' : '미등록'}
								tone={member.registered ? 'success' : 'warning'}
							/>
						</div>
						<div>
							{#if member.mainLane}
								<strong class="font-mono text-sm tabular-nums text-slate-100">
									{formatMmr(member.laneMmr[member.mainLane])}
								</strong>
								<span class="ml-2 text-[10px] font-semibold text-slate-500">
									{laneLabel[member.mainLane]}
								</span>
							{:else}
								<span class="text-xs text-slate-500">미설정</span>
							{/if}
						</div>
						<span class="text-xs text-slate-500">{lastActiveLabel(member.lastActiveAt)}</span>
						<div class="flex justify-end gap-2">
							<button
								type="button"
								disabled={mutationPending}
								onclick={() => openAdjustment(member)}
								class="inline-flex h-8 items-center gap-1.5 border border-[var(--color-app-border)] px-2 text-xs font-semibold text-slate-300 hover:border-cyan-400/35 hover:text-cyan-100 disabled:opacity-35"
							>
								<Gauge size={13} aria-hidden="true" /> MMR
							</button>
							<button
								type="button"
								disabled={mutationPending || cannotDeactivateSelf(member)}
								onclick={() => toggleMember(member)}
								aria-label={statusActionLabel(member)}
								title={statusActionLabel(member)}
								class={`grid size-8 place-items-center border ${
									member.status === 'ACTIVE'
										? 'border-rose-300/25 text-rose-200 hover:bg-rose-400/10'
										: 'border-emerald-300/25 text-emerald-200 hover:bg-emerald-400/10'
								} disabled:opacity-35`}
							>
								{#if member.status === 'ACTIVE'}
									<UserX size={14} aria-hidden="true" />
								{:else}
									<UserCheck size={14} aria-hidden="true" />
								{/if}
							</button>
						</div>
					</div>
				{:else}
					<div class="px-4 py-10 text-center text-sm text-slate-600">검색 결과 없음</div>
				{/each}
			</div>
		</div>
	</div>
</section>

<Dialog.Root bind:open={adjustmentOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">MMR 수동 조정</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
					aria-label="MMR 조정 닫기"
				>
					<X size={16} />
				</Dialog.Close>
			</header>
			{#if selectedMember}
				<div class="grid gap-4 p-4">
					<div
						class="flex min-w-0 items-center justify-between gap-3 border border-[var(--color-app-border-muted)] bg-slate-950/20 px-3 py-2"
					>
						<span class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">
								{selectedMember.discordName}
							</strong>
							<small class="block truncate text-[10px] text-slate-600">
								{selectedMember.riotId ?? 'Riot 계정 미연결'}
							</small>
						</span>
						<span class="shrink-0 text-[10px] font-semibold text-slate-500">감사 로그 기록</span>
					</div>
					<div class="grid gap-3">
						<label
							class="grid min-w-0 gap-1.5 text-xs font-semibold text-slate-500 sm:grid-cols-[5rem_minmax(0,1fr)] sm:items-center"
						>
							<span>포지션</span>
							<select
								bind:value={selectedLane}
								class="h-10 w-full min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							>
								<option value="" disabled>포지션 선택</option>
								{#each LANES as lane (lane)}
									<option value={lane}>
										{laneLabel[lane]} · {formatMmr(selectedMember.laneMmr[lane])}
									</option>
								{/each}
							</select>
						</label>
						<label
							class="grid min-w-0 gap-1.5 text-xs font-semibold text-slate-500 sm:grid-cols-[5rem_minmax(0,1fr)] sm:items-center"
						>
							<span>증감값</span>
							<input
								type="number"
								bind:value={adjustmentDelta}
								class="h-10 w-full min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label
							class="grid min-w-0 gap-1.5 text-xs font-semibold text-slate-500 sm:grid-cols-[5rem_minmax(0,1fr)] sm:items-center"
						>
							<span>조정 사유</span>
							<input
								bind:value={adjustmentNote}
								maxlength="500"
								placeholder="예: 누락된 경기 결과 반영"
								class="h-10 w-full min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-400/45"
							/>
						</label>
					</div>
					{#if errorMessage}
						<p class="text-xs text-rose-200" role="alert">{errorMessage}</p>
					{/if}
				</div>
				<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
					<Dialog.Close
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
					>
						취소
					</Dialog.Close>
					<button
						type="button"
						disabled={!canAdjust || submitting}
						onclick={applyAdjustment}
						class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
					>
						{submitting ? '적용 중' : '조정 적용'}
					</button>
				</footer>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
