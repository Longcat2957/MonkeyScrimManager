<script lang="ts">
	import { Search, SquareCheckBig, X } from 'lucide-svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import type { DummyLane, DummyUser } from '$lib/contracts/dummyUsers';

	interface Props {
		users: DummyUser[];
		selectedIds: string[];
		onSelectionChange: (userIds: string[]) => void;
		disabled?: boolean;
		maxSelected?: number;
		requiredGuildId?: string | null;
	}

	let {
		users,
		selectedIds,
		onSelectionChange,
		disabled = false,
		maxSelected = 100,
		requiredGuildId = null
	}: Props = $props();
	let query = $state('');
	let lane = $state<'ALL' | DummyLane>('ALL');
	let guildId = $state('ALL');

	const selected = $derived(new Set(selectedIds));
	const guildOptions = $derived(
		Array.from(
			new Map(
				users.flatMap((user) =>
					scopedMemberships(user).map((membership) => [membership.guildId, membership.guildName])
				)
			).entries()
		).map(([id, name]) => ({ id, name }))
	);
	const filteredUsers = $derived(
		users.filter((user) => {
			const normalizedQuery = query.trim().toLocaleLowerCase();
			const matchesQuery =
				!normalizedQuery ||
				user.displayName.toLocaleLowerCase().includes(normalizedQuery) ||
				user.stableKey.toLocaleLowerCase().includes(normalizedQuery);
			const matchesLane =
				lane === 'ALL' ||
				scopedMemberships(user).some((membership) => membership.mainLane === lane);
			const matchesGuild =
				guildId === 'ALL' ||
				scopedMemberships(user).some((membership) => membership.guildId === guildId);
			const matchesRequiredGuild =
				requiredGuildId === null ||
				user.guildMemberships.some(
					(membership) => membership.guildId === requiredGuildId && membership.status === 'ACTIVE'
				);
			return (
				user.state === 'ACTIVE' &&
				matchesRequiredGuild &&
				matchesQuery &&
				matchesLane &&
				matchesGuild
			);
		})
	);
	const allVisibleSelected = $derived(
		filteredUsers.length > 0 && filteredUsers.every((user) => selected.has(user.userId))
	);

	function toggle(userId: string) {
		if (disabled) return;
		const next = new SvelteSet(selectedIds);
		if (next.has(userId)) next.delete(userId);
		else if (next.size < maxSelected) next.add(userId);
		onSelectionChange([...next]);
	}

	function toggleVisible() {
		if (disabled) return;
		const next = new SvelteSet(selectedIds);
		if (allVisibleSelected) {
			for (const user of filteredUsers) next.delete(user.userId);
		} else {
			for (const user of filteredUsers) {
				if (next.size >= maxSelected) break;
				next.add(user.userId);
			}
		}
		onSelectionChange([...next]);
	}

	function laneNames(user: DummyUser): string {
		const lanes = new Set(
			scopedMemberships(user).flatMap((membership) =>
				membership.mainLane ? [membership.mainLane] : []
			)
		);
		return lanes.size > 0 ? [...lanes].join(' / ') : '라인 미지정';
	}

	function scopedMemberships(user: DummyUser) {
		return requiredGuildId === null
			? user.guildMemberships
			: user.guildMemberships.filter((membership) => membership.guildId === requiredGuildId);
	}
</script>

<section class="overflow-hidden border border-[var(--color-app-border-muted)]">
	<div
		class={`grid gap-2 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-2 ${requiredGuildId ? 'sm:grid-cols-[minmax(0,1fr)_112px]' : 'sm:grid-cols-[minmax(0,1fr)_112px_150px]'}`}
	>
		<label
			class="flex h-9 min-w-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3"
		>
			<Search size={14} class="shrink-0 text-slate-500" aria-hidden="true" />
			<input
				bind:value={query}
				placeholder="이름 또는 stable key"
				aria-label="더미 사용자 검색"
				class="min-w-0 flex-1 bg-transparent text-xs text-slate-100 outline-none placeholder:text-slate-600"
			/>
			{#if query}
				<button
					type="button"
					onclick={() => (query = '')}
					aria-label="검색어 지우기"
					class="grid size-6 shrink-0 place-items-center text-slate-500 hover:text-slate-200"
				>
					<X size={13} />
				</button>
			{/if}
		</label>
		<select
			bind:value={lane}
			aria-label="포지션 필터"
			class="h-9 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2 text-xs text-slate-300 outline-none"
		>
			<option value="ALL">전체 라인</option>
			<option value="TOP">TOP</option>
			<option value="JUNGLE">JUNGLE</option>
			<option value="MID">MID</option>
			<option value="BOTTOM">BOTTOM</option>
			<option value="SUPPORT">SUPPORT</option>
		</select>
		{#if !requiredGuildId}
			<select
				bind:value={guildId}
				aria-label="길드 필터"
				class="h-9 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2 text-xs text-slate-300 outline-none"
			>
				<option value="ALL">전체 길드</option>
				{#each guildOptions as guild (guild.id)}
					<option value={guild.id}>{guild.name}</option>
				{/each}
			</select>
		{/if}
	</div>

	<div
		class="flex h-9 items-center justify-between border-b border-[var(--color-app-border-muted)] px-3"
	>
		<span class="text-[11px] text-slate-500">{selectedIds.length}/{maxSelected}명 선택</span>
		<button
			type="button"
			disabled={disabled || filteredUsers.length === 0}
			onclick={toggleVisible}
			class="inline-flex h-7 items-center gap-1.5 px-2 text-[11px] font-semibold text-cyan-100 hover:bg-cyan-400/10 disabled:opacity-35"
		>
			<SquareCheckBig size={13} aria-hidden="true" />
			{allVisibleSelected ? '표시 항목 해제' : '표시 항목 선택'}
		</button>
	</div>

	<div class="max-h-72 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto">
		{#each filteredUsers as user (user.userId)}
			<label
				class={`grid min-h-12 cursor-pointer grid-cols-[22px_minmax(0,1fr)_auto] items-center gap-2 px-3 py-2 hover:bg-[var(--color-app-hover)] ${
					disabled ? 'pointer-events-none opacity-50' : ''
				}`}
			>
				<input
					type="checkbox"
					checked={selected.has(user.userId)}
					disabled={disabled || (!selected.has(user.userId) && selectedIds.length >= maxSelected)}
					onchange={() => toggle(user.userId)}
					class="size-4 accent-cyan-400"
				/>
				<span class="min-w-0">
					<strong class="block truncate text-xs text-slate-100">{user.displayName}</strong>
					<small class="block truncate text-[10px] text-slate-600">{user.stableKey}</small>
				</span>
				<span class="text-right text-[10px] text-slate-500">
					<strong class="block font-semibold text-slate-300">{laneNames(user)}</strong>
					{scopedMemberships(user).length}개 길드
				</span>
			</label>
		{:else}
			<p class="px-4 py-8 text-center text-xs text-slate-600">선택 가능한 사용자가 없습니다.</p>
		{/each}
	</div>
</section>
