<script lang="ts">
	import { CircleSlash, Dice5, Gavel, PackageOpen, Swords, Trophy, Undo2 } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import { championIconUrl } from '$lib/datadragon';
	import {
		mockQuickRecordResults,
		type QuickRecordLane,
		type QuickRecordSearchResult
	} from '$lib/fixtures/quickRecordSearch';
	import { laneLabel, type MockAuctionMember, type MockAuctionTeam } from '$lib/fixtures/workflow';
	import QuickRecordChampionRow from '$lib/components/fab/QuickRecordChampionRow.svelte';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		initialTeams: MockAuctionTeam[];
		initialQueue: MockAuctionMember[];
		profileIconVersion?: string | null;
	}

	type AuctionActionKind = 'BID' | 'MANUAL' | 'UNSOLD' | 'UNSOLD_ASSIGN';

	interface RevertableAuctionAction {
		kind: AuctionActionKind;
		teamId: string | null;
		member: MockAuctionMember;
		previousPoints: number | null;
	}

	let { initialTeams, initialQueue, profileIconVersion = null }: Props = $props();

	const quickRecordLaneLabel: Record<MockAuctionMember['role'], QuickRecordLane> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '원딜',
		SUPPORT: '서폿'
	};

	const positionIconPath: Record<MockAuctionMember['role'], string> = {
		TOP: '/riot/positions/top.svg',
		JUNGLE: '/riot/positions/jungle.svg',
		MID: '/riot/positions/mid.svg',
		BOTTOM: '/riot/positions/bottom.svg',
		SUPPORT: '/riot/positions/support.svg'
	};
	const teamMemberSlots = [1, 2, 3, 4, 5] as const;

	let teams = $state<MockAuctionTeam[]>(untrack(() => cloneTeams(initialTeams)));
	let candidateQueue = $state<MockAuctionMember[]>(untrack(() => cloneMembers(initialQueue)));
	let currentTarget = $state<MockAuctionMember | null>(null);
	let unsold = $state<MockAuctionMember[]>([]);
	let bidInputs = $state<Record<string, string>>({});
	let draggedTargetId = $state<string | null>(null);
	let overTeamId = $state<string | null>(null);
	let showTargetDetails = $state(false);
	let lastRevertableAction = $state<RevertableAuctionAction | null>(null);

	const expectedTotal = $derived(teams.length * 5);
	const placedCount = $derived(teams.reduce((sum, team) => sum + team.members.length, 0));
	const captainCount = $derived(teams.length);
	const bidPlacedCount = $derived(Math.max(0, placedCount - captainCount));
	const recruitPoolSize = $derived(initialQueue.length + captainCount);
	const remainingCount = $derived(candidateQueue.length + (currentTarget ? 1 : 0));
	const allPlaced = $derived(placedCount >= expectedTotal);
	const canDraw = $derived(!currentTarget && !allPlaced && candidateQueue.length > 0);
	const canRevert = $derived(Boolean(lastRevertableAction) && !currentTarget);

	function cloneMembers(source: MockAuctionMember[]): MockAuctionMember[] {
		return source.map((member) => ({ ...member }));
	}

	function cloneTeams(source: MockAuctionTeam[]): MockAuctionTeam[] {
		return source.map((team) => ({
			...team,
			initialPoints: team.initialPoints ?? 1000,
			members: team.members.map((member) => ({
				...member,
				acquiredVia: member.acquiredVia ?? (member.price === 0 ? 'CAPTAIN' : 'BID'),
				acquiredAtPoints: member.acquiredAtPoints ?? (member.price > 0 ? member.price : null)
			}))
		}));
	}

	function drawNext() {
		if (!canDraw) return;
		const [next, ...rest] = candidateQueue;
		if (!next) return;
		currentTarget = next;
		candidateQueue = rest;
		resetTargetInteraction();
	}

	function cancelDraw() {
		if (!currentTarget) return;
		unsold = [...unsold, currentTarget];
		lastRevertableAction = {
			kind: 'UNSOLD',
			teamId: null,
			member: currentTarget,
			previousPoints: null
		};
		currentTarget = null;
		resetTargetInteraction();
	}

	function isUnsoldMemberId(memberId: string | null): boolean {
		return Boolean(memberId && unsold.some((member) => member.id === memberId));
	}

	function canReceiveDrop(team: MockAuctionTeam): boolean {
		if (team.members.length >= 5) return false;
		return Boolean(currentTarget || isUnsoldMemberId(draggedTargetId));
	}

	function bidNumber(teamId: string): number | null {
		const raw = bidInputs[teamId]?.trim() ?? '';
		if (!raw) return null;
		const value = Number(raw);
		if (!Number.isFinite(value) || value < 0) return null;
		return Math.floor(value);
	}

	function adjustBid(team: MockAuctionTeam, delta: number) {
		if (!currentTarget || team.members.length >= 5) return;
		const current = bidNumber(team.id) ?? 0;
		const next = Math.max(0, Math.min(team.points, current + delta));
		bidInputs = {
			...bidInputs,
			[team.id]: String(next)
		};
	}

	function canFinalizeBid(team: MockAuctionTeam): boolean {
		const points = bidNumber(team.id);
		if (!currentTarget || points === null) return false;
		if (team.members.length >= 5) return false;
		return team.points >= points;
	}

	function finalizeBid(teamId: string) {
		if (!currentTarget) return;
		const team = teams.find((item) => item.id === teamId);
		if (!team) return;
		const points = bidNumber(team.id);
		if (points === null) {
			return;
		}
		if (team.members.length >= 5) {
			return;
		}
		if (team.points < points) {
			return;
		}

		const target = currentTarget;
		const acquired: MockAuctionMember = {
			...target,
			price: points,
			acquiredVia: 'BID',
			acquiredAtPoints: points
		};

		teams = teams.map((item) =>
			item.id === teamId
				? {
						...item,
						points: item.points - points,
						members: [...item.members, acquired]
					}
				: item
		);
		lastRevertableAction = {
			kind: 'BID',
			teamId,
			member: target,
			previousPoints: team.points
		};
		currentTarget = null;
		resetTargetInteraction();
	}

	function assignCurrentTargetToTeam(teamId: string) {
		if (!currentTarget) return;
		const team = teams.find((item) => item.id === teamId);
		if (!team) return;
		if (team.members.length >= 5) {
			return;
		}

		const target = currentTarget;
		const acquired: MockAuctionMember = {
			...target,
			price: 0,
			acquiredVia: 'MANUAL',
			acquiredAtPoints: null
		};

		teams = teams.map((item) =>
			item.id === teamId ? { ...item, members: [...item.members, acquired] } : item
		);
		lastRevertableAction = {
			kind: 'MANUAL',
			teamId,
			member: target,
			previousPoints: team.points
		};
		currentTarget = null;
		resetTargetInteraction();
	}

	function assignUnsoldMemberToTeam(teamId: string, memberId: string) {
		const team = teams.find((item) => item.id === teamId);
		const target = unsold.find((member) => member.id === memberId);
		if (!team || !target || team.members.length >= 5) return;

		const acquired: MockAuctionMember = {
			...target,
			price: 0,
			acquiredVia: 'MANUAL',
			acquiredAtPoints: null
		};

		teams = teams.map((item) =>
			item.id === teamId ? { ...item, members: [...item.members, acquired] } : item
		);
		unsold = unsold.filter((member) => member.id !== memberId);
		lastRevertableAction = {
			kind: 'UNSOLD_ASSIGN',
			teamId,
			member: target,
			previousPoints: team.points
		};
		resetTargetInteraction();
	}

	function restoreUnsoldToTarget(memberId: string) {
		if (currentTarget) return;
		const target = unsold.find((member) => member.id === memberId);
		if (!target) return;
		unsold = unsold.filter((member) => member.id !== memberId);
		currentTarget = target;
		lastRevertableAction = null;
		resetTargetInteraction();
	}

	function revertLastAction() {
		if (!lastRevertableAction || currentTarget) return;
		const action = lastRevertableAction;

		if (action.kind === 'UNSOLD') {
			unsold = unsold.filter((member) => member.id !== action.member.id);
			currentTarget = action.member;
		} else if (action.kind === 'UNSOLD_ASSIGN' && action.teamId) {
			teams = teams.map((team) =>
				team.id === action.teamId
					? {
							...team,
							points: action.previousPoints ?? team.points,
							members: team.members.filter((member) => member.id !== action.member.id)
						}
					: team
			);
			unsold = [action.member, ...unsold];
		} else if (action.teamId) {
			teams = teams.map((team) =>
				team.id === action.teamId
					? {
							...team,
							points: action.previousPoints ?? team.points,
							members: team.members.filter((member) => member.id !== action.member.id)
						}
					: team
			);
			currentTarget = action.member;
		}

		lastRevertableAction = null;
		resetTargetInteraction();
	}

	function resetTargetInteraction() {
		bidInputs = {};
		showTargetDetails = false;
		clearDragState();
	}

	function startTargetDrag(event: globalThis.DragEvent) {
		if (!currentTarget || !event.dataTransfer) return;
		draggedTargetId = currentTarget.id;
		event.dataTransfer.setData('text/plain', currentTarget.id);
		event.dataTransfer.effectAllowed = 'move';
	}

	function startUnsoldDrag(event: globalThis.DragEvent, member: MockAuctionMember) {
		if (!event.dataTransfer) return;
		draggedTargetId = member.id;
		event.dataTransfer.setData('text/plain', member.id);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		draggedTargetId = null;
		overTeamId = null;
	}

	function handleTeamDragOver(event: globalThis.DragEvent, team: MockAuctionTeam) {
		if (!event.dataTransfer || !canReceiveDrop(team)) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = canReceiveDrop(team) ? 'move' : 'none';
		overTeamId = team.id;
	}

	function handleTeamDrop(event: globalThis.DragEvent, team: MockAuctionTeam) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		const userId = event.dataTransfer.getData('text/plain');
		if (currentTarget && userId === currentTarget.id) {
			assignCurrentTargetToTeam(team.id);
		} else if (isUnsoldMemberId(userId)) {
			assignUnsoldMemberToTeam(team.id, userId);
		}
		clearDragState();
	}

	function memberSlotClass(team: MockAuctionTeam, slot: number): string {
		const filled = slot <= team.members.length;
		if (team.members.length >= 5 && filled) return 'border-emerald-300 bg-emerald-300';
		return filled ? 'border-cyan-300 bg-cyan-300' : 'border-slate-700 bg-slate-900';
	}

	function teamDropClass(team: MockAuctionTeam): string {
		if (overTeamId === team.id) {
			return canReceiveDrop(team)
				? 'rounded-md border-cyan-300/65 bg-cyan-400/12 ring-1 ring-cyan-300/35'
				: 'rounded-md border-red-300/55 bg-red-500/10 ring-1 ring-red-300/35';
		}
		return 'border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]';
	}

	function acquisitionLabel(member: MockAuctionMember): string {
		if (member.acquiredVia === 'CAPTAIN' || member.price === 0) return '팀장';
		if (member.acquiredVia === 'MANUAL') return '수동';
		return `${member.acquiredAtPoints ?? member.price}P`;
	}

	function acquisitionBadgeClass(member: MockAuctionMember): string {
		if (member.acquiredVia === 'MANUAL') return 'bg-cyan-400/10 text-xs text-cyan-100';
		if (member.acquiredVia === 'CAPTAIN' || member.price === 0) {
			return 'bg-amber-300/10 text-xs text-amber-100';
		}
		return 'bg-slate-950/35 text-sm font-bold text-slate-100 tabular-nums';
	}

	function quickRecordTemplate(member: MockAuctionMember): QuickRecordSearchResult {
		const [midTemplate, jungleTemplate] = mockQuickRecordResults;
		const template = member.role === 'JUNGLE' ? (jungleTemplate ?? midTemplate) : midTemplate;

		return {
			...template,
			displayName: member.name,
			riotId: `${member.name}#MSM`,
			profileIconId: member.profileIconId ?? template.profileIconId,
			custom: {
				...template.custom,
				mainLane: quickRecordLaneLabel[member.role],
				laneMmr: template.custom.laneMmr.map((lane) =>
					lane.lane === quickRecordLaneLabel[member.role] ? { ...lane, mmr: member.mmr } : lane
				)
			}
		};
	}

	function recentRecordSummary(games: QuickRecordSearchResult['custom']['recentGames']): string {
		const wins = games.filter((game) => game.result === '승').length;
		return `${wins}승 ${games.length - wins}패`;
	}

	function shortRecentRecord(games: QuickRecordSearchResult['custom']['recentGames']): string {
		return recentRecordSummary(games.slice(0, 8));
	}
</script>

<div class="grid gap-2">
	<section class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
		<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-2 xl:grid-cols-4">
			<div class="bg-[var(--color-app-surface)] px-3 py-3">
				<p class="text-xs text-slate-500">입찰 풀</p>
				<p class="mt-1 text-2xl font-semibold text-slate-100 tabular-nums">
					{recruitPoolSize - captainCount}
				</p>
			</div>
			<div class="bg-[var(--color-app-surface)] px-3 py-3">
				<p class="text-xs text-slate-500">배치</p>
				<p class="mt-1 text-2xl font-semibold text-emerald-100 tabular-nums">
					{bidPlacedCount}/{recruitPoolSize - captainCount}
				</p>
			</div>
			<div class="bg-[var(--color-app-surface)] px-3 py-3">
				<p class="text-xs text-slate-500">잔여</p>
				<p class="mt-1 text-2xl font-semibold text-cyan-100 tabular-nums">{remainingCount}</p>
			</div>
			<div class="bg-[var(--color-app-surface)] px-3 py-3">
				<p class="text-xs text-slate-500">유찰</p>
				<p class="mt-1 text-2xl font-semibold text-amber-100 tabular-nums">{unsold.length}</p>
			</div>
		</div>
		<div class="px-3 py-2">
			<div class="h-1.5 bg-slate-800">
				<div
					class="h-full bg-cyan-300"
					style={`width: ${(placedCount / expectedTotal) * 100}%`}
				></div>
			</div>
		</div>
	</section>

	<div class="grid gap-2 xl:grid-cols-[minmax(25rem,35rem)_minmax(0,1fr)]">
		<div class="grid gap-2 xl:h-[56rem] xl:grid-rows-[minmax(0,8fr)_minmax(0,2fr)]">
			<section
				class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] border-y border-cyan-400/30 bg-[var(--color-app-surface)]"
			>
				<div
					class="flex items-center justify-between border-b border-[var(--color-app-border-muted)] px-3 py-2"
				>
					<div class="flex items-center gap-2">
						<p class="text-base font-semibold text-slate-100">현재 매물</p>
						{#if currentTarget}
							<span class="size-2 rounded-full bg-emerald-300"></span>
						{/if}
					</div>
					<div class="flex flex-wrap items-center justify-end gap-1.5">
						<button
							type="button"
							class="inline-flex h-9 items-center gap-1.5 rounded-sm px-3 text-sm font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100 disabled:cursor-not-allowed disabled:opacity-40"
							disabled={!canRevert}
							onclick={revertLastAction}
						>
							<Undo2 size={14} />
							되돌리기
						</button>
						<button
							type="button"
							class={`inline-flex h-9 items-center rounded-sm px-3 text-sm font-semibold disabled:cursor-not-allowed disabled:opacity-40 ${
								showTargetDetails
									? 'bg-cyan-400/12 text-cyan-100'
									: 'text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100'
							}`}
							disabled={!currentTarget}
							onclick={() => {
								showTargetDetails = !showTargetDetails;
							}}
						>
							상세보기
						</button>
						<button
							type="button"
							class="inline-flex h-9 items-center gap-1.5 rounded-sm px-3 text-sm font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100 disabled:cursor-not-allowed disabled:opacity-40"
							disabled={!canDraw}
							onclick={drawNext}
						>
							<Dice5 size={14} />
							다음 후보
						</button>
					</div>
				</div>

				<div class="grid min-h-0 grid-rows-[minmax(0,1fr)_auto] gap-3 overflow-hidden p-3">
					{#if currentTarget}
						<div class="grid min-h-0 content-start gap-3 overflow-hidden">
							{#if showTargetDetails}
								<div
									class={`cursor-grab border px-4 py-4 transition active:cursor-grabbing ${
										draggedTargetId === currentTarget.id
											? 'rounded-md border-cyan-300/60 bg-cyan-400/15 opacity-70'
											: 'rounded-sm border-cyan-400/30 bg-cyan-400/5'
									}`}
									draggable="true"
									ondragstart={startTargetDrag}
									ondragend={clearDragState}
									role="group"
									aria-label={`현재 경매 매물 ${currentTarget.name}`}
								>
									<div class="flex items-start gap-4">
										<PlayerAvatar
											name={currentTarget.name}
											role={currentTarget.role}
											avatarUrl={currentTarget.avatarUrl}
											profileIconId={currentTarget.profileIconId}
											{profileIconVersion}
											size="md"
										/>
										<div class="min-w-0 flex-1">
											<p class="truncate text-2xl font-semibold text-slate-50">
												{currentTarget.name}
											</p>
											<div class="mt-2 flex flex-wrap items-center gap-1.5">
												<span
													class="inline-flex items-center gap-1 border border-cyan-400/25 bg-cyan-400/10 px-2 py-1 text-sm font-semibold text-cyan-100"
												>
													<PositionIcon role={currentTarget.role} size="sm" />
													{laneLabel[currentTarget.role]}
												</span>
												{#if currentTarget.riotTier}
													<span
														class="border border-amber-300/25 bg-amber-300/10 px-2 py-1 text-sm font-semibold text-amber-100"
													>
														{currentTarget.riotTier}
													</span>
												{/if}
												<span class="px-2 py-1 text-sm font-semibold text-slate-300">
													MMR {currentTarget.mmr}
												</span>
											</div>
										</div>
									</div>
								</div>
							{:else}
								{@const record = quickRecordTemplate(currentTarget)}
								<div
									class={`relative isolate min-h-[30rem] cursor-grab overflow-hidden border px-4 py-5 transition active:cursor-grabbing xl:min-h-[32rem] ${
										draggedTargetId === currentTarget.id
											? 'rounded-md border-cyan-300/60 bg-cyan-400/15 opacity-70'
											: 'rounded-sm border-cyan-400/30 bg-slate-950/35'
									}`}
									draggable="true"
									ondragstart={startTargetDrag}
									ondragend={clearDragState}
									role="group"
									aria-label={`현재 경매 매물 ${currentTarget.name}`}
								>
									<img
										src={positionIconPath[currentTarget.role]}
										alt=""
										class="pointer-events-none absolute top-0 right-0 -z-10 size-64 translate-x-8 -translate-y-6 opacity-[0.065]"
										loading="lazy"
									/>
									<div
										class="pointer-events-none absolute inset-x-0 bottom-0 -z-10 h-24 bg-cyan-400/[0.045]"
									></div>
									<div class="grid h-full content-between gap-5">
										<div class="flex items-start justify-between gap-3">
											<span
												class="inline-flex items-center gap-1 border border-cyan-400/25 bg-cyan-400/10 px-2 py-1 text-sm font-semibold text-cyan-100"
											>
												<PositionIcon role={currentTarget.role} size="sm" />
												{laneLabel[currentTarget.role]}
											</span>
											{#if currentTarget.riotTier}
												<span
													class="border border-amber-300/25 bg-amber-300/10 px-2 py-1 text-sm font-semibold text-amber-100"
												>
													{currentTarget.riotTier}
												</span>
											{/if}
										</div>

										<div class="grid justify-items-center gap-3 text-center">
											<PlayerAvatar
												name={currentTarget.name}
												role={currentTarget.role}
												avatarUrl={currentTarget.avatarUrl}
												profileIconId={currentTarget.profileIconId}
												{profileIconVersion}
												size="md"
											/>
											<div class="min-w-0">
												<p class="max-w-72 truncate text-3xl font-semibold text-slate-50">
													{currentTarget.name}
												</p>
												<p class="mt-1 text-base font-semibold text-slate-300">
													MMR {currentTarget.mmr}
												</p>
											</div>
										</div>

										<div class="grid gap-2">
											<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
												<div class="bg-slate-950/70 px-3 py-2 text-center">
													<p class="text-[11px] text-slate-500">승률</p>
													<p class="mt-1 text-base font-semibold text-cyan-100">
														{record.custom.winRate}%
													</p>
												</div>
												<div class="bg-slate-950/70 px-3 py-2 text-center">
													<p class="text-[11px] text-slate-500">승패</p>
													<p class="mt-1 text-base font-semibold text-slate-100">
														{record.custom.wins}-{record.custom.losses}
													</p>
												</div>
												<div class="bg-slate-950/70 px-3 py-2 text-center">
													<p class="text-[11px] text-slate-500">최근</p>
													<p class="mt-1 text-base font-semibold text-slate-100">
														{shortRecentRecord(record.custom.recentGames)}
													</p>
												</div>
											</div>
										</div>
									</div>
								</div>
							{/if}

							{#if showTargetDetails}
								{@const record = quickRecordTemplate(currentTarget)}
								<div
									class="grid max-h-[34rem] gap-3 overflow-y-auto bg-[var(--color-app-bg-soft)] p-3 pr-[13px]"
								>
									<section class="grid gap-3">
										<div class="flex items-center justify-between gap-3">
											<div class="flex items-center gap-2">
												<Trophy size={16} class="text-cyan-100" />
												<p class="text-sm font-semibold text-slate-100">내전 전적</p>
											</div>
											<p class="text-xs font-semibold text-slate-500">
												{record.custom.wins}승 {record.custom.losses}패
											</p>
										</div>

										<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
											<div class="bg-[var(--color-app-surface)] p-2">
												<p class="text-[11px] text-slate-500">내전</p>
												<p class="mt-1 text-sm font-semibold text-slate-100">
													{record.custom.games}게임
												</p>
											</div>
											<div class="bg-[var(--color-app-surface)] p-2">
												<p class="text-[11px] text-slate-500">승률</p>
												<p class="mt-1 text-sm font-semibold text-cyan-100">
													{record.custom.winRate}%
												</p>
											</div>
											<div class="bg-[var(--color-app-surface)] p-2">
												<p class="text-[11px] text-slate-500">주력 포지션</p>
												<p class="mt-1 text-sm font-semibold text-slate-100">
													{record.custom.mainLane}
												</p>
											</div>
										</div>

										<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-5">
											{#each record.custom.laneMmr as lane (lane.lane)}
												<div class="bg-[var(--color-app-surface)] p-2">
													<div class="flex items-center justify-between gap-2">
														<span class="text-xs font-semibold text-slate-400">{lane.lane}</span>
														<span class="text-[11px] font-semibold text-slate-500">
															{lane.games}게임
														</span>
													</div>
													<p class="mt-1 text-sm font-semibold text-slate-100 tabular-nums">
														{lane.mmr}
													</p>
												</div>
											{/each}
										</div>

										<div class="grid gap-2">
											{#each record.custom.champions.slice(0, 3) as champion (champion.id)}
												<QuickRecordChampionRow
													{champion}
													dataDragonVersion={record.dataDragonVersion}
													iconSize="md"
												/>
											{/each}
										</div>
									</section>

									<section class="grid gap-2">
										<div class="flex items-center justify-between gap-3">
											<p class="text-xs font-semibold text-slate-400">최근 게임</p>
											<p class="text-xs font-semibold text-slate-500">
												{recentRecordSummary(record.custom.recentGames)}
											</p>
										</div>
										<div
											class="flex gap-0.5"
											aria-label={`최근 ${record.custom.recentGames.length}게임 ${recentRecordSummary(record.custom.recentGames)}`}
										>
											{#each record.custom.recentGames as game (game.id)}
												<span
													class={`h-1.5 min-w-0 flex-1 ${
														game.result === '승' ? 'bg-cyan-300/80' : 'bg-rose-300/70'
													}`}
												></span>
											{/each}
										</div>
										<div class="grid max-h-44 gap-1 overflow-y-auto pr-[5px]">
											{#each record.custom.recentGames as game (game.id)}
												<div
													class="grid grid-cols-[auto_1fr_auto] items-center gap-2 bg-[var(--color-app-surface)] p-2"
												>
													<img
														src={championIconUrl(game.champion.imageFull, record.dataDragonVersion)}
														alt={game.champion.name}
														class="size-8 object-cover"
														loading="lazy"
													/>
													<div class="min-w-0">
														<p class="truncate text-xs font-semibold text-slate-100">
															{game.label}
														</p>
														<p class="truncate text-[11px] font-medium text-slate-500">
															{game.champion.name} · {game.lane} · {game.playedAt}
														</p>
													</div>
													<span
														class={`text-xs font-semibold ${
															game.result === '승' ? 'text-cyan-100' : 'text-rose-200'
														}`}
													>
														{game.result}
													</span>
												</div>
											{/each}
										</div>
									</section>

									<section class="grid gap-3">
										<div class="flex items-center justify-between gap-3">
											<div class="flex items-center gap-2">
												<Swords size={16} class="text-amber-100" />
												<p class="text-sm font-semibold text-slate-100">Riot 전적</p>
											</div>
											<p class="text-xs font-semibold text-slate-500">{record.riot.refreshedAt}</p>
										</div>

										<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
											<div class="bg-[var(--color-app-surface)] p-2">
												<p class="text-[11px] text-slate-500">{record.riot.rankType}</p>
												<p class="mt-1 text-sm font-semibold text-amber-100">{record.riot.tier}</p>
											</div>
											<div class="bg-[var(--color-app-surface)] p-2">
												<p class="text-[11px] text-slate-500">최근</p>
												<p class="mt-1 text-sm font-semibold text-slate-100">
													{record.riot.games}게임
												</p>
											</div>
											<div class="bg-[var(--color-app-surface)] p-2">
												<p class="text-[11px] text-slate-500">승률</p>
												<p class="mt-1 text-sm font-semibold text-amber-100">
													{record.riot.winRate}%
												</p>
											</div>
										</div>

										<div class="grid gap-px bg-[var(--color-app-border-muted)]">
											{#each record.riot.recentChampions.slice(0, 3) as champion (champion.id)}
												<QuickRecordChampionRow
													{champion}
													dataDragonVersion={record.dataDragonVersion}
													tone="riot"
												/>
											{/each}
										</div>
									</section>
								</div>
							{/if}
						</div>

						<div class="flex justify-end">
							<button
								type="button"
								class="inline-flex h-10 items-center gap-1.5 rounded-sm px-3.5 text-sm font-semibold text-amber-100 hover:bg-amber-300/10"
								onclick={cancelDraw}
							>
								<CircleSlash size={14} />
								유찰
							</button>
						</div>
					{:else if allPlaced}
						<div class="grid min-h-80 place-items-center text-center">
							<div>
								<Gavel class="mx-auto text-emerald-200" size={28} />
								<p class="mt-3 text-base font-semibold text-emerald-100">브래킷 시작 가능</p>
								<p class="mt-1 text-sm text-slate-500">모든 팀 정원이 찼습니다.</p>
							</div>
						</div>
					{:else}
						<div
							class="grid min-h-80 place-items-center border border-dashed border-slate-700 text-center text-base text-slate-500"
						>
							<div>
								<PackageOpen class="mx-auto" size={24} />
								<p class="mt-2 font-semibold">후보 대기</p>
								<p class="mt-1 text-sm">다음 후보를 뽑아 입찰을 시작합니다.</p>
							</div>
						</div>
					{/if}
				</div>
			</section>

			<section
				class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
			>
				<div
					class="flex items-center justify-between border-b border-[var(--color-app-border-muted)] px-3 py-2"
				>
					<p class="text-base font-semibold text-slate-100">유찰</p>
					<p class="text-xs text-slate-500">팀 카드로 드래그</p>
				</div>
				<div role="list" class="grid min-h-0 content-start gap-1.5 overflow-y-auto p-2">
					{#each unsold as member (member.id)}
						<div
							role="listitem"
							draggable="true"
							ondragstart={(event) => startUnsoldDrag(event, member)}
							ondragend={clearDragState}
							class={`flex min-h-11 min-w-0 cursor-grab items-center gap-2 rounded-sm border border-amber-300/10 bg-amber-300/[0.045] px-2 py-1.5 text-left text-sm text-slate-100 hover:border-amber-200/20 hover:bg-amber-300/[0.075] active:cursor-grabbing ${
								draggedTargetId === member.id ? 'opacity-60' : ''
							}`}
							aria-label={`${member.name} 유찰 후보`}
						>
							<PlayerAvatar
								name={member.name}
								role={member.role}
								avatarUrl={member.avatarUrl}
								profileIconId={member.profileIconId}
								{profileIconVersion}
								size="sm"
							/>
							<span class="min-w-0 flex-1">
								<span class="block truncate text-sm font-semibold">{member.name}</span>
								<span class="block truncate text-[11px] text-slate-500">
									{member.riotTier ?? '-'} · MMR {member.mmr} · {laneLabel[member.role]}
								</span>
							</span>
							<button
								type="button"
								class="shrink-0 rounded-sm px-2 py-1 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10 disabled:cursor-not-allowed disabled:opacity-40"
								disabled={Boolean(currentTarget)}
								onclick={(event) => {
									event.stopPropagation();
									restoreUnsoldToTarget(member.id);
								}}
							>
								재등록
							</button>
						</div>
					{:else}
						<p class="py-2 text-center text-base text-slate-500">아직 유찰 없음</p>
					{/each}
				</div>
			</section>
		</div>

		<section
			class="grid min-h-0 gap-px border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)] lg:grid-cols-2 lg:grid-rows-2 xl:h-[56rem]"
		>
			{#each teams as team (team.id)}
				<div
					class={`grid min-h-0 grid-rows-[auto_minmax(0,1fr)] border px-3 py-3 transition ${teamDropClass(team)}`}
					ondragover={(event) => handleTeamDragOver(event, team)}
					ondragleave={() => {
						if (overTeamId === team.id) overTeamId = null;
					}}
					ondrop={(event) => handleTeamDrop(event, team)}
					role="group"
					aria-label={`${team.name} 입찰 팀`}
				>
					<div class="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-2">
						<div class="min-w-0">
							<div class="flex min-w-0 items-center gap-1.5">
								<span
									class="shrink-0 border border-cyan-400/25 px-1.5 py-0.5 text-xs font-semibold text-cyan-100"
								>
									{team.name}
								</span>
								<p class="min-w-0 truncate text-base font-semibold text-slate-100">
									{team.captain}
								</p>
							</div>
						</div>
						<div
							class="shrink-0 border border-amber-300/30 px-2.5 py-1 text-base font-bold text-amber-100 tabular-nums"
						>
							{team.points}P
						</div>
					</div>

					<div class="mt-2 grid min-h-0 w-full min-w-0 grid-rows-[auto_minmax(0,1fr)] gap-3">
						<div class="grid w-full min-w-0 gap-3">
							<div class="grid w-full min-w-0 gap-1.5">
								<div class="flex items-center justify-between text-xs text-slate-500">
									<span>인원 {team.members.length}/5</span>
								</div>
								<div class="grid w-full min-w-0 grid-cols-5 gap-1">
									{#each teamMemberSlots as slot (slot)}
										<span class={`h-2 border ${memberSlotClass(team, slot)}`}></span>
									{/each}
								</div>
							</div>

							<div
								class="grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-1"
							>
								<input
									type="text"
									inputmode="numeric"
									pattern="[0-9]*"
									class="h-9 w-full min-w-0 rounded-none border border-[var(--color-app-border-muted)] bg-slate-950/35 px-3 text-right text-base font-semibold text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-300/50"
									placeholder="입찰가"
									disabled={!currentTarget || team.members.length >= 5}
									value={bidInputs[team.id] ?? ''}
									oninput={(event) => {
										bidInputs = {
											...bidInputs,
											[team.id]: event.currentTarget.value
										};
									}}
									aria-label={`${team.name} 입찰가`}
								/>
								<button
									type="button"
									class="inline-flex h-9 items-center border border-cyan-400/25 px-2.5 text-sm font-bold text-cyan-100 hover:border-cyan-300/45 disabled:cursor-not-allowed disabled:opacity-40"
									disabled={!currentTarget || team.members.length >= 5}
									title="좌클릭 +10 / 우클릭 -10"
									aria-label={`${team.name} 입찰가 10점 조절`}
									onclick={() => adjustBid(team, 10)}
									oncontextmenu={(event) => {
										event.preventDefault();
										adjustBid(team, -10);
									}}
								>
									±10
								</button>
								<button
									type="button"
									class="inline-flex h-9 items-center gap-1.5 rounded-sm bg-emerald-400/14 px-3 text-sm font-semibold text-emerald-100 hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
									disabled={!canFinalizeBid(team)}
									onclick={() => finalizeBid(team.id)}
								>
									<Gavel size={14} />
									낙찰
								</button>
							</div>
						</div>

						<div
							role="list"
							class="grid min-h-0 w-full min-w-0 content-start gap-1 overflow-x-hidden overflow-y-auto overscroll-contain"
							aria-label={`${team.name} 낙찰 멤버`}
						>
							{#each team.members as member (member.id)}
								<div
									role="listitem"
									class="flex min-h-11 w-full items-center gap-2.5 border border-transparent bg-slate-950/20 px-2 py-1.5 text-sm"
								>
									<PlayerAvatar
										name={member.name}
										role={member.role}
										avatarUrl={member.avatarUrl}
										profileIconId={member.profileIconId}
										{profileIconVersion}
										size="sm"
									/>
									<div class="min-w-0 flex-1">
										<p class="truncate text-base font-semibold text-slate-100">{member.name}</p>
										<p class="mt-0.5 truncate text-xs font-medium text-slate-500">
											{member.riotTier ?? '-'} · MMR {member.mmr} · {laneLabel[member.role]}
										</p>
									</div>
									<span class={`shrink-0 px-2 py-0.5 ${acquisitionBadgeClass(member)}`}>
										{acquisitionLabel(member)}
									</span>
								</div>
							{:else}
								<p class="py-2 text-center text-base text-slate-500">아직 없음</p>
							{/each}
						</div>
					</div>
				</div>
			{/each}
		</section>
	</div>
</div>
