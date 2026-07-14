<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { Check, LoaderCircle, Lock, RotateCcw, X } from 'lucide-svelte';
	import type {
		AuctionMatchLineupAssignCommand,
		AuctionMatchLineupLockCommand,
		AuctionMatchLineupUnlockCommand,
		AuctionMatchLineupMember,
		AuctionMatchLineupSnapshot,
		AuctionMatchLineupTeam
	} from '$lib/contracts/auctionMatchLineup';
	import type { ChampionPreferenceLane } from '$lib/contracts/workflow';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		snapshot: AuctionMatchLineupSnapshot;
		busy?: boolean;
		onAssign?: (command: AuctionMatchLineupAssignCommand) => void | Promise<void>;
		onLock?: (command: AuctionMatchLineupLockCommand) => void | Promise<void>;
		onUnlock?: (command: AuctionMatchLineupUnlockCommand) => void | Promise<void>;
	}

	let { snapshot, busy = false, onAssign, onLock, onUnlock }: Props = $props();

	const lanes: ChampionPreferenceLane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const laneLabels: Record<ChampionPreferenceLane, string> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '원딜',
		SUPPORT: '서폿'
	};

	const leftTeam = $derived(
		snapshot.teams.find((team) => team.id === snapshot.match.leftTeamId) ?? null
	);
	const rightTeam = $derived(
		snapshot.teams.find((team) => team.id === snapshot.match.rightTeamId) ?? null
	);
	const matchTeams = $derived(
		[leftTeam, rightTeam].filter((team): team is AuctionMatchLineupTeam => team !== null)
	);
	const filledSlotCount = $derived(snapshot.lineups.length);
	const lineupComplete = $derived(
		matchTeams.length === 2 &&
			matchTeams.every((team) => lanes.every((lane) => assignmentFor(team.id, lane) !== null))
	);
	const canManageLineup = $derived(
		Boolean(onAssign) &&
			snapshot.permissions.canManageLineup &&
			!snapshot.match.lineupsLocked &&
			snapshot.match.status === 'LINEUP_PENDING' &&
			!busy
	);
	const canLockLineup = $derived(
		Boolean(onLock) && canManageLineup && lineupComplete && !snapshot.match.lineupsLocked
	);
	const canUnlockLineup = $derived(
		Boolean(onUnlock) &&
			snapshot.permissions.canUnlockLineup &&
			snapshot.match.lineupsLocked &&
			!busy
	);
	const rulesLabel = $derived(
		`BO${snapshot.match.bestOf} · ${snapshot.match.hardFearless ? '하드 피어리스' : '일반'}`
	);

	let unlockDialogOpen = $state(false);

	function assignmentFor(teamId: string, lane: ChampionPreferenceLane) {
		return (
			snapshot.lineups.find(
				(assignment) => assignment.teamId === teamId && assignment.lane === lane
			) ?? null
		);
	}

	function memberForAssignment(
		team: AuctionMatchLineupTeam,
		lane: ChampionPreferenceLane
	): AuctionMatchLineupMember | null {
		const assignment = assignmentFor(team.id, lane);
		if (!assignment) return null;
		return team.members.find((member) => member.teamMemberId === assignment.teamMemberId) ?? null;
	}

	function memberInitial(displayName: string): string {
		return displayName.trim().slice(0, 1).toLocaleUpperCase('ko-KR') || '?';
	}

	function laneLabel(lane: ChampionPreferenceLane | null): string {
		return lane ? laneLabels[lane] : '미설정';
	}

	function memberDetail(member: AuctionMatchLineupMember): string {
		const mmr =
			member.mmrAtAcquisition === null
				? 'MMR 미집계'
				: `MMR ${Math.round(member.mmrAtAcquisition).toLocaleString('ko-KR')}`;
		return `신청 라인 · ${laneLabel(member.declaredLane)} · ${mmr}`;
	}

	function isAssignedElsewhere(
		teamId: string,
		lane: ChampionPreferenceLane,
		teamMemberId: string
	): boolean {
		const member = snapshot.teams
			.find((team) => team.id === teamId)
			?.members.find((candidate) => candidate.teamMemberId === teamMemberId);
		if (!member) return true;
		return snapshot.lineups.some(
			(assignment) =>
				(assignment.teamMemberId === teamMemberId || assignment.userId === member.userId) &&
				!(assignment.teamId === teamId && assignment.lane === lane)
		);
	}

	async function assignMember(
		teamId: string,
		lane: ChampionPreferenceLane,
		teamMemberId: string | null
	): Promise<void> {
		if (!canManageLineup || !onAssign) return;
		const current = assignmentFor(teamId, lane);
		if (current?.teamMemberId === teamMemberId) return;
		if (teamMemberId !== null && isAssignedElsewhere(teamId, lane, teamMemberId)) return;

		try {
			await onAssign({
				expectedRevision: snapshot.match.revision,
				teamId,
				lane,
				teamMemberId
			});
		} catch {
			// The parent owns the user-visible API error and keeps the snapshot authoritative.
		}
	}

	function selectMember(teamId: string, lane: ChampionPreferenceLane, value: string) {
		const teamMemberId = value || null;
		void assignMember(teamId, lane, teamMemberId);
	}

	async function lockLineup(): Promise<void> {
		if (!canLockLineup || !onLock) return;
		try {
			await onLock({ expectedRevision: snapshot.match.revision });
		} catch {
			// The parent owns the user-visible API error and keeps the snapshot authoritative.
		}
	}

	async function unlockLineup(): Promise<void> {
		if (!canUnlockLineup || !onUnlock) return;
		try {
			await onUnlock({ expectedRevision: snapshot.match.revision });
		} finally {
			unlockDialogOpen = false;
		}
	}
</script>

<div class="grid gap-2 pb-2" aria-busy={busy}>
	<div
		class="flex flex-col gap-2 border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] px-3 py-3 sm:flex-row sm:items-center sm:justify-between"
	>
		<div class="min-w-0">
			<p class="truncate text-base font-semibold text-slate-100">{snapshot.match.label} 라인업</p>
			<p class="mt-1 text-xs font-medium text-slate-500">{rulesLabel}</p>
		</div>
		<span
			class={`shrink-0 text-xs font-semibold ${
				snapshot.match.lineupsLocked ? 'text-emerald-200' : 'text-slate-500'
			}`}
		>
			{snapshot.match.lineupsLocked ? '라인업 확정됨' : `라인업 ${filledSlotCount}/10`}
		</span>
	</div>

	{#if matchTeams.length === 2}
		<div class="grid gap-2 xl:grid-cols-2">
			{#each matchTeams as team (team.id)}
				<section
					class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
				>
					<header
						class="flex min-h-14 items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2.5"
					>
						<div class="min-w-0">
							<h2 class="truncate text-base font-semibold text-slate-100">{team.teamName}</h2>
							<p class="mt-0.5 text-xs font-medium text-slate-500">
								{team.members.length}명 참가자
							</p>
						</div>
					</header>

					<div class="grid gap-1.5 p-3">
						{#each lanes as lane (lane)}
							{@const assignment = assignmentFor(team.id, lane)}
							{@const assignedMember = memberForAssignment(team, lane)}
							<div
								class={`grid min-h-16 grid-cols-[auto_minmax(0,1fr)_minmax(8rem,11rem)_2.25rem] items-center gap-2 border px-2.5 py-2 ${
									assignedMember
										? 'border-cyan-300/18 bg-cyan-400/[0.045]'
										: 'border-[var(--color-app-border-muted)] bg-slate-950/20'
								}`}
							>
								<PositionIcon role={lane} size="md" />
								<div class="min-w-0">
									{#if assignedMember}
										<div class="flex min-w-0 items-center gap-2">
											<span
												class="grid size-8 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-xs font-semibold text-slate-300"
											>
												{memberInitial(assignedMember.displayName)}
											</span>
											<div class="min-w-0">
												<p class="truncate text-sm font-semibold text-slate-100">
													{assignedMember.displayName}
												</p>
												<p class="truncate text-[11px] text-slate-500">
													{memberDetail(assignedMember)}
												</p>
											</div>
										</div>
									{:else}
										<p class="truncate text-sm font-semibold text-slate-600">
											{laneLabels[lane]} 미배치
										</p>
									{/if}
								</div>

								<select
									class="h-9 min-w-0 border border-[var(--color-app-border-muted)] bg-slate-950/40 px-2 text-xs font-semibold text-slate-200 outline-none focus:border-cyan-300/50 disabled:cursor-not-allowed disabled:opacity-45"
									value={assignment?.teamMemberId ?? ''}
									disabled={!canManageLineup}
									onchange={(event) => selectMember(team.id, lane, event.currentTarget.value)}
									aria-label={`${team.teamName} ${laneLabels[lane]} 선수 선택`}
								>
									<option value="">선수 선택</option>
									{#each team.members as member (member.teamMemberId)}
										<option
											value={member.teamMemberId}
											disabled={isAssignedElsewhere(team.id, lane, member.teamMemberId)}
										>
											{member.displayName} · {laneLabel(member.declaredLane)}
										</option>
									{/each}
								</select>

								{#if assignment}
									<button
										type="button"
										class="grid size-9 place-items-center border border-rose-300/25 bg-rose-500/10 text-rose-200 enabled:hover:border-rose-300/45 enabled:hover:bg-rose-500/16 disabled:cursor-not-allowed disabled:opacity-40"
										disabled={!canManageLineup}
										onclick={() => void assignMember(team.id, lane, null)}
										aria-label={`${team.teamName} ${laneLabels[lane]} 배치 해제`}
									>
										<X size={16} aria-hidden="true" />
									</button>
								{:else}
									<span class="size-9" aria-hidden="true"></span>
								{/if}
							</div>
						{/each}
					</div>
				</section>
			{/each}
		</div>
	{:else}
		<div
			class="grid min-h-32 place-items-center border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] px-4 text-center text-sm font-medium text-slate-500"
		>
			이 경기의 양 팀 정보를 불러오지 못했습니다.
		</div>
	{/if}

	{#if snapshot.match.lineupsLocked}
		<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
			<div
				class="inline-flex h-11 items-center justify-center gap-2 border border-emerald-300/25 bg-emerald-400/8 px-4 text-sm font-semibold text-emerald-100"
			>
				<Check size={16} aria-hidden="true" /> 라인업 확정됨
			</div>
			{#if canUnlockLineup}
				<button
					type="button"
					class="inline-flex h-11 items-center justify-center gap-2 border border-amber-300/35 bg-amber-400/10 px-4 text-sm font-semibold text-amber-100 hover:bg-amber-400/16"
					onclick={() => {
						unlockDialogOpen = true;
					}}
				>
					<RotateCcw size={16} aria-hidden="true" /> 라인업 다시 편성
				</button>
			{/if}
		</div>
	{:else}
		<button
			type="button"
			class="inline-flex h-11 items-center justify-center gap-2 border border-emerald-300/35 bg-emerald-400/14 px-4 text-sm font-semibold text-emerald-100 enabled:hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
			disabled={!canLockLineup}
			onclick={() => void lockLineup()}
		>
			{#if busy}
				<LoaderCircle size={16} class="animate-spin" aria-hidden="true" />
			{:else}
				<Lock size={16} aria-hidden="true" />
			{/if}
			라인업 확정
		</button>
	{/if}
</div>

<Dialog.Root bind:open={unlockDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<Dialog.Title class="text-sm font-semibold text-slate-50">라인업 다시 편성</Dialog.Title>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				현재 10인 배치는 유지한 채 라인업 확정만 해제합니다. 해제 후에는 선수를 다시 배치할 수
				있습니다. 확정된 게임 결과가 있으면 서버가 이 작업을 거부합니다.
			</Dialog.Description>
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={busy}
					class="inline-flex h-8 items-center px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={!canUnlockLineup}
					class="inline-flex h-8 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
					onclick={() => void unlockLineup()}
				>
					{#if busy}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
					{:else}
						<RotateCcw size={14} aria-hidden="true" />
					{/if}
					다시 편성하기
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
