<script lang="ts">
	import { Dice5, PackageOpen, Plus, RotateCcw } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import type { MockAuctionMember, MockAuctionTeam } from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		initialTeams: MockAuctionTeam[];
		initialQueue: MockAuctionMember[];
		profileIconVersion?: string | null;
	}

	let { initialTeams, initialQueue, profileIconVersion = null }: Props = $props();

	let teams = $state<MockAuctionTeam[]>(untrack(() => cloneTeams(initialTeams)));
	let queue = $state<MockAuctionMember[]>(untrack(() => [...initialQueue]));
	let currentTarget = $state<MockAuctionMember | null>(untrack(() => initialQueue[0] ?? null));
	let draggedTargetId = $state<string | null>(null);
	let overTeamId = $state<string | null>(null);
	let lastAction = $state('현재 대상을 팀 원장으로 끌어 배치할 수 있습니다.');
	let showDetails = $state(false);

	const initialPointBudget = 1000;
	const remainingQueue = $derived(queue.filter((member) => member.id !== currentTarget?.id));
	const placedCount = $derived(teams.reduce((sum, team) => sum + team.members.length, 0));
	const expectedCount = $derived(teams.length * 5);
	const allPlaced = $derived(placedCount >= expectedCount);

	function cloneTeams(source: MockAuctionTeam[]): MockAuctionTeam[] {
		return source.map((team) => ({
			...team,
			members: team.members.map((member) => ({ ...member }))
		}));
	}

	function resetBoard() {
		teams = cloneTeams(initialTeams);
		queue = [...initialQueue];
		currentTarget = initialQueue[0] ?? null;
		draggedTargetId = null;
		overTeamId = null;
		lastAction = '경매 배치 상태를 초기화했습니다.';
	}

	function drawNext() {
		currentTarget = remainingQueue[0] ?? null;
		draggedTargetId = null;
		overTeamId = null;
		lastAction = currentTarget
			? `${currentTarget.name} 님이 현재 대상입니다.`
			: '남은 대상이 없습니다.';
	}

	function canAssign(team: MockAuctionTeam): boolean {
		if (!currentTarget) return false;
		if (team.members.length >= 5) return false;
		return team.points >= currentTarget.price;
	}

	function assignToTeam(teamId: string) {
		if (!currentTarget) return;
		const team = teams.find((item) => item.id === teamId);
		if (!team || !canAssign(team)) {
			lastAction = '정원 또는 포인트 조건 때문에 배치할 수 없습니다.';
			return;
		}

		teams = teams.map((item) =>
			item.id === teamId
				? {
						...item,
						points: item.points - currentTarget!.price,
						members: [...item.members, currentTarget!]
					}
				: item
		);
		queue = queue.filter((member) => member.id !== currentTarget?.id);
		lastAction = `${currentTarget.name} 님을 ${team.name}에 배치했습니다.`;
		currentTarget = null;
		draggedTargetId = null;
		overTeamId = null;
	}

	function startTargetDrag(event: globalThis.DragEvent) {
		if (!currentTarget || !event.dataTransfer) return;
		draggedTargetId = currentTarget.id;
		event.dataTransfer.setData('text/plain', currentTarget.id);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		draggedTargetId = null;
		overTeamId = null;
	}

	function handleTeamDragOver(event: globalThis.DragEvent, team: MockAuctionTeam) {
		if (!event.dataTransfer || !currentTarget) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = canAssign(team) ? 'move' : 'none';
		overTeamId = team.id;
	}

	function handleTeamDrop(event: globalThis.DragEvent, team: MockAuctionTeam) {
		if (!event.dataTransfer || !currentTarget) return;
		event.preventDefault();
		const userId = event.dataTransfer.getData('text/plain');
		if (userId === currentTarget.id) assignToTeam(team.id);
		clearDragState();
	}

	function pointPercent(team: MockAuctionTeam): number {
		return Math.max(0, Math.min(100, Math.round((team.points / initialPointBudget) * 100)));
	}

	function memberPercent(team: MockAuctionTeam): number {
		return Math.max(0, Math.min(100, Math.round((team.members.length / 5) * 100)));
	}

	function captainMember(team: MockAuctionTeam): MockAuctionMember | null {
		return team.members.find((member) => member.price === 0) ?? team.members[0] ?? null;
	}

	function teamPanelClass(team: MockAuctionTeam): string {
		if (overTeamId === team.id) {
			return canAssign(team)
				? 'rounded-md border-cyan-300/60 bg-cyan-400/10 ring-1 ring-cyan-300/35'
				: 'rounded-md border-red-300/50 bg-red-500/10 ring-1 ring-red-300/35';
		}
		return currentTarget && canAssign(team)
			? 'rounded-sm border-cyan-400/25 bg-cyan-400/5'
			: 'border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]';
	}
</script>

<div class="grid gap-2 xl:grid-cols-[18rem_28rem_minmax(0,1fr)]">
	<aside class="grid gap-2">
		<section
			class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] p-2"
		>
			<div class="grid grid-cols-2 gap-1.5">
				<div class="border border-[var(--color-app-border-muted)] px-2 py-2">
					<p class="text-[11px] text-slate-500">배치</p>
					<p class="mt-1 text-lg font-semibold text-slate-100">{placedCount}/{expectedCount}</p>
				</div>
				<div class="border border-[var(--color-app-border-muted)] px-2 py-2">
					<p class="text-[11px] text-slate-500">대기</p>
					<p class="mt-1 text-lg font-semibold text-slate-100">{remainingQueue.length}</p>
				</div>
			</div>
			<div class="mt-2 h-1.5 bg-slate-800">
				<div
					class="h-full bg-cyan-300"
					style={`width: ${(placedCount / expectedCount) * 100}%`}
				></div>
			</div>
		</section>

		<section class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
			<div
				class="flex items-center justify-between border-b border-[var(--color-app-border-muted)] px-3 py-2"
			>
				<p class="text-sm font-semibold text-slate-100">상태</p>
				<button
					type="button"
					class={`h-7 border px-2 text-xs font-semibold ${
						showDetails
							? 'rounded-md border-cyan-400/35 bg-cyan-400/10 text-cyan-100'
							: 'rounded-sm border-transparent text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-cyan-100'
					}`}
					onclick={() => {
						showDetails = !showDetails;
					}}
				>
					상세
				</button>
			</div>
			<p class="px-3 py-2 text-xs leading-5 text-slate-400">{lastAction}</p>
			{#if showDetails}
				<div class="border-t border-[var(--color-app-border-muted)] px-3 py-2">
					<p class="text-xs font-semibold text-slate-500">대기열</p>
					<div class="mt-1 grid gap-1">
						{#each remainingQueue as member (member.id)}
							<div class="flex items-center justify-between gap-2 text-xs">
								<span class="truncate text-slate-300">{member.name}</span>
								<span class="inline-flex shrink-0 items-center gap-1 text-slate-500">
									<PositionIcon role={member.role} size="sm" />
									<span>{member.price}P</span>
								</span>
							</div>
						{:else}
							<p class="text-xs text-slate-500">대기 인원이 없습니다.</p>
						{/each}
					</div>
				</div>
			{/if}
		</section>
	</aside>

	<section class="border-y border-cyan-400/30 bg-[var(--color-app-surface)]">
		<div
			class="flex items-center justify-between border-b border-[var(--color-app-border-muted)] px-3 py-2"
		>
			<div class="flex items-center gap-2">
				<p class="text-sm font-semibold text-slate-100">현재 매물</p>
				{#if currentTarget}
					<span class="size-2 rounded-full bg-emerald-300"></span>
				{/if}
			</div>
			<div class="flex items-center gap-1.5">
				<button
					type="button"
					class="inline-flex h-8 items-center gap-1.5 rounded-sm px-2.5 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100"
					disabled={Boolean(currentTarget) || allPlaced}
					onclick={drawNext}
				>
					<Dice5 size={14} />
					다음
				</button>
				<button
					type="button"
					class="inline-flex h-8 items-center gap-1.5 rounded-sm px-2.5 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100"
					onclick={resetBoard}
				>
					<RotateCcw size={14} />
				</button>
			</div>
		</div>

		<div class="p-2">
			{#if currentTarget}
				<div
					class={`cursor-grab border px-3 py-3 active:cursor-grabbing ${
						draggedTargetId === currentTarget.id
							? 'rounded-md border-cyan-300/60 bg-cyan-400/15'
							: 'rounded-sm border-cyan-400/30 bg-cyan-400/5'
					}`}
					draggable="true"
					ondragstart={startTargetDrag}
					ondragend={clearDragState}
					role="group"
					aria-label={`현재 경매 대상 ${currentTarget.name}`}
				>
					<div class="flex items-center gap-3">
						<PlayerAvatar
							name={currentTarget.name}
							role={currentTarget.role}
							avatarUrl={currentTarget.avatarUrl}
							profileIconId={currentTarget.profileIconId}
							{profileIconVersion}
							size="md"
						/>
						<div class="min-w-0 flex-1">
							<p class="truncate text-lg font-semibold text-slate-50">{currentTarget.name}</p>
							{#if showDetails}
								<p class="mt-0.5 inline-flex items-center gap-1 text-xs text-slate-500">
									<PositionIcon role={currentTarget.role} size="sm" />
									<span>{currentTarget.mmr}</span>
								</p>
							{:else}
								<p class="mt-0.5 text-xs text-slate-500">팀으로 드롭</p>
							{/if}
						</div>
						<span class="border border-cyan-400/30 px-2 py-1 text-xs font-semibold text-cyan-100">
							{currentTarget.price}P
						</span>
					</div>
				</div>
			{:else if allPlaced}
				<div class="grid min-h-28 place-items-center text-sm font-semibold text-emerald-100">
					모든 인원 배치 완료
				</div>
			{:else}
				<div
					class="grid min-h-28 place-items-center border border-dashed border-slate-700 text-sm text-slate-500"
				>
					<PackageOpen size={20} />
					<span>다음 인원 대기</span>
				</div>
			{/if}
		</div>
	</section>

	<section
		class="grid gap-px border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)] lg:grid-cols-2"
	>
		{#each teams as team (team.id)}
			{@const captain = captainMember(team)}
			<div
				class={`border px-3 py-3 transition ${teamPanelClass(team)}`}
				ondragover={(event) => handleTeamDragOver(event, team)}
				ondragleave={() => {
					if (overTeamId === team.id) overTeamId = null;
				}}
				ondrop={(event) => handleTeamDrop(event, team)}
				role="group"
				aria-label={`${team.name} 팀 원장`}
			>
				<div class="flex items-center gap-2">
					<div
						class="grid size-11 shrink-0 place-items-center border border-amber-300/30 text-xs font-bold text-amber-100"
					>
						{team.points}P
					</div>
					{#if captain}
						<PlayerAvatar
							name={captain.name}
							role={captain.role}
							avatarUrl={captain.avatarUrl}
							profileIconId={captain.profileIconId}
							{profileIconVersion}
						/>
					{/if}
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-1.5">
							<span
								class="border border-cyan-400/25 px-1.5 py-0.5 text-[11px] font-semibold text-cyan-100"
							>
								{team.name}
							</span>
							<span class="border border-amber-300/25 px-1.5 py-0.5 text-[11px] text-amber-100"
								>팀장</span
							>
						</div>
						<p class="mt-1 truncate text-sm font-semibold text-slate-100">{team.captain}</p>
					</div>
				</div>

				<div class="mt-3 grid gap-1">
					<div class="flex items-center justify-between text-[11px] text-slate-500">
						<span>인원 {team.members.length}/5</span>
						<span>{canAssign(team) ? '배치 가능' : '배치 불가'}</span>
					</div>
					<div class="grid grid-cols-5 gap-1">
						{#each [1, 2, 3, 4, 5] as slot (slot)}
							<span
								class={`h-2 border ${
									slot <= team.members.length
										? 'border-cyan-300 bg-cyan-300'
										: 'border-slate-700 bg-slate-900'
								}`}
							></span>
						{/each}
					</div>
					{#if showDetails}
						<div class="h-1.5 bg-slate-800">
							<div class="h-full bg-amber-300" style={`width: ${pointPercent(team)}%`}></div>
						</div>
						<div class="h-1.5 bg-slate-800">
							<div class="h-full bg-cyan-300" style={`width: ${memberPercent(team)}%`}></div>
						</div>
					{/if}
				</div>

				<div class="mt-3 grid gap-1">
					{#each team.members as member (member.id)}
						<div class="flex min-h-7 items-center gap-1.5 text-xs">
							<PlayerAvatar
								name={member.name}
								role={member.role}
								avatarUrl={member.avatarUrl}
								profileIconId={member.profileIconId}
								{profileIconVersion}
								size="xs"
							/>
							<span class="min-w-0 flex-1 truncate text-slate-200">{member.name}</span>
							<span class="shrink-0 text-slate-500">
								{member.price === 0 ? '팀장' : `${member.price}P`}
							</span>
						</div>
					{:else}
						<p class="py-1 text-center text-sm text-slate-500">아직 없음</p>
					{/each}
				</div>

				<button
					type="button"
					class="mt-3 inline-flex h-8 items-center gap-1.5 rounded-sm px-2.5 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100 disabled:cursor-not-allowed disabled:opacity-40"
					disabled={!canAssign(team)}
					onclick={() => assignToTeam(team.id)}
				>
					<Plus size={14} />
					배치
				</button>
			</div>
		{/each}
	</section>
</div>
