<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { CircleSlash, Dice5, Gavel, PackageOpen, Swords, Trophy, Undo2, X } from 'lucide-svelte';
	import type { ChampionCatalog } from '$lib/datadragon';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import type {
		AuctionTournamentDraftCandidate,
		AuctionTournamentDraftCommand,
		AuctionTournamentDraftSnapshot,
		AuctionTournamentTeam,
		AuctionTournamentTeamMember
	} from '$lib/contracts/auctionTournamentDraft';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		snapshot: AuctionTournamentDraftSnapshot;
		onCommand: (command: AuctionTournamentDraftCommand) => void | Promise<void>;
		submitting?: boolean;
		championCatalog?: ChampionCatalog | null;
	}

	let { snapshot, onCommand, submitting = false, championCatalog = null }: Props = $props();

	const laneLabels = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '바텀',
		SUPPORT: '서포트'
	} as const;
	const teamMemberSlots = [1, 2, 3, 4, 5] as const;

	let bidInputs = $state<Record<string, string>>({});
	let draggedTargetId = $state<string | null>(null);
	let overTeamId = $state<string | null>(null);
	let showTargetDetails = $state(false);
	let revertDialogOpen = $state(false);

	const teams = $derived(snapshot.teams);
	const currentTarget = $derived(snapshot.draft.currentTarget);
	const currentTargetProfile = $derived(snapshot.draft.currentTargetProfile);
	const unsold = $derived(snapshot.draft.unsold);
	const queuedCandidates = $derived(
		snapshot.candidates.filter((candidate) => candidate.state === 'QUEUED')
	);
	const placedCount = $derived(teams.reduce((count, team) => count + team.members.length, 0));
	const expectedTotal = $derived(teams.length * 5);
	const assignedCount = $derived(
		snapshot.candidates.filter((candidate) => candidate.state === 'ASSIGNED').length
	);
	const candidatePoolSize = $derived(snapshot.candidates.length);
	const remainingCount = $derived(
		snapshot.candidates.filter(
			(candidate) => candidate.state === 'QUEUED' || candidate.state === 'ACTIVE'
		).length
	);
	const allPlaced = $derived(expectedTotal > 0 && placedCount >= expectedTotal);
	const canManageDraft = $derived(snapshot.permissions.canManageDraft && !submitting);
	const canDraw = $derived(
		canManageDraft && !currentTarget && !allPlaced && queuedCandidates.length > 0
	);
	const canRevert = $derived(snapshot.permissions.canRevertLast && !submitting);
	const revertLabel = $derived(
		snapshot.tournament.status === 'BRACKET_SETUP' ? '입찰 단계로 되돌리기' : '마지막 입찰 되돌리기'
	);

	function playerInitial(displayName: string): string {
		return displayName.trim().slice(0, 1).toLocaleUpperCase('ko-KR') || '?';
	}

	function playerLaneLabel(
		player: Pick<AuctionTournamentDraftCandidate | AuctionTournamentTeamMember, 'declaredLane'>
	): string {
		return player.declaredLane ? laneLabels[player.declaredLane] : '라인 미설정';
	}

	function mmrLabel(mmr: number | null): string {
		return mmr === null ? '미집계' : Math.round(mmr).toLocaleString('ko-KR');
	}

	function currentTargetRole(): keyof typeof laneLabels {
		return currentTarget?.declaredLane ?? currentTargetProfile?.mainLane ?? 'MID';
	}

	function winRate(games: number, wins: number): string {
		if (games === 0) return '-';
		return `${Math.round((wins / games) * 100)}%`;
	}

	function recordWinRate(): string {
		if (!currentTargetProfile) return '-';
		return winRate(currentTargetProfile.summary.games, currentTargetProfile.summary.wins);
	}

	function recentRecordSummary(): string {
		const recentGames = currentTargetProfile?.recentGames ?? [];
		const wins = recentGames.filter((game) => game.result === 'WIN').length;
		return `${wins}승 ${recentGames.length - wins}패`;
	}

	function playedAtLabel(value: string): string {
		const playedAt = new Date(value);
		if (Number.isNaN(playedAt.getTime())) return '';
		return new Intl.DateTimeFormat('ko-KR', {
			month: 'numeric',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		}).format(playedAt);
	}

	function championFor(championId: string) {
		return championCatalog?.byId[championId] ?? null;
	}

	async function submit(command: AuctionTournamentDraftCommand): Promise<boolean> {
		const allowed = command.kind === 'REVERT_LAST' ? canRevert : canManageDraft;
		if (!allowed) return false;
		try {
			await onCommand(command);
			resetTransientState();
			return true;
		} catch {
			// The parent owns the safe API error message and keeps the server snapshot unchanged.
			return false;
		}
	}

	function drawNext() {
		void submit({ kind: 'DRAW_NEXT', expectedRevision: snapshot.tournament.revision });
	}

	function markUnsold() {
		if (!currentTarget) return;
		void submit({
			kind: 'MARK_UNSOLD',
			memberId: currentTarget.userId,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function isUnsoldMemberId(memberId: string | null): boolean {
		return Boolean(memberId && unsold.some((member) => member.userId === memberId));
	}

	function canReceiveDrop(team: AuctionTournamentTeam): boolean {
		if (!canManageDraft || team.members.length >= 5) return false;
		return Boolean(currentTarget || isUnsoldMemberId(draggedTargetId));
	}

	function bidNumber(teamId: string): number | null {
		const raw = bidInputs[teamId]?.trim() ?? '';
		if (!raw) return null;
		const value = Number(raw);
		if (!Number.isFinite(value) || value < 0) return null;
		return Math.floor(value);
	}

	function adjustBid(team: AuctionTournamentTeam, delta: number) {
		if (!currentTarget || !canManageDraft || team.members.length >= 5) return;
		const current = bidNumber(team.id) ?? 0;
		const next = Math.max(0, Math.min(team.currentPoints, current + delta));
		bidInputs = { ...bidInputs, [team.id]: String(next) };
	}

	function canFinalizeBid(team: AuctionTournamentTeam): boolean {
		const points = bidNumber(team.id);
		return Boolean(
			canManageDraft &&
			currentTarget &&
			points !== null &&
			team.members.length < 5 &&
			team.currentPoints >= points
		);
	}

	function finalizeBid(team: AuctionTournamentTeam) {
		if (!currentTarget) return;
		const points = bidNumber(team.id);
		if (points === null || !canFinalizeBid(team)) return;
		void submit({
			kind: 'AWARD_BID',
			memberId: currentTarget.userId,
			teamId: team.id,
			points,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function assignCurrentTargetToTeam(team: AuctionTournamentTeam) {
		if (!currentTarget || !canReceiveDrop(team)) return;
		void submit({
			kind: 'ASSIGN_CURRENT',
			memberId: currentTarget.userId,
			teamId: team.id,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function assignUnsoldMemberToTeam(team: AuctionTournamentTeam, memberId: string) {
		if (!isUnsoldMemberId(memberId) || !canReceiveDrop(team)) return;
		void submit({
			kind: 'ASSIGN_UNSOLD',
			memberId,
			teamId: team.id,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function restoreUnsoldToTarget(memberId: string) {
		if (currentTarget || !canManageDraft || !isUnsoldMemberId(memberId)) return;
		void submit({
			kind: 'RESTORE_UNSOLD',
			memberId,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function openRevertDialog() {
		if (!canRevert) return;
		revertDialogOpen = true;
	}

	async function revertLastAction() {
		if (!canRevert) return;
		const applied = await submit({
			kind: 'REVERT_LAST',
			expectedRevision: snapshot.tournament.revision
		});
		if (applied) revertDialogOpen = false;
	}

	function resetTransientState() {
		bidInputs = {};
		showTargetDetails = false;
		clearDragState();
	}

	function startTargetDrag(event: globalThis.DragEvent) {
		if (!currentTarget || !event.dataTransfer || !canManageDraft) return;
		draggedTargetId = currentTarget.userId;
		event.dataTransfer.setData('text/plain', currentTarget.userId);
		event.dataTransfer.effectAllowed = 'move';
	}

	function startUnsoldDrag(event: globalThis.DragEvent, member: AuctionTournamentDraftCandidate) {
		if (!event.dataTransfer || !canManageDraft) return;
		draggedTargetId = member.userId;
		event.dataTransfer.setData('text/plain', member.userId);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		draggedTargetId = null;
		overTeamId = null;
	}

	function handleTeamDragOver(event: globalThis.DragEvent, team: AuctionTournamentTeam) {
		if (!event.dataTransfer || !canReceiveDrop(team)) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = 'move';
		overTeamId = team.id;
	}

	function handleTeamDrop(event: globalThis.DragEvent, team: AuctionTournamentTeam) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		const memberId = event.dataTransfer.getData('text/plain');
		if (currentTarget && memberId === currentTarget.userId) {
			assignCurrentTargetToTeam(team);
		} else if (isUnsoldMemberId(memberId)) {
			assignUnsoldMemberToTeam(team, memberId);
		}
		clearDragState();
	}

	function memberSlotClass(team: AuctionTournamentTeam, slot: number): string {
		const filled = slot <= team.members.length;
		if (team.members.length >= 5 && filled) return 'border-emerald-300 bg-emerald-300';
		return filled ? 'border-cyan-300 bg-cyan-300' : 'border-slate-700 bg-slate-900';
	}

	function teamDropClass(team: AuctionTournamentTeam): string {
		if (overTeamId === team.id) {
			return canReceiveDrop(team)
				? 'border-cyan-300/65 bg-cyan-400/12 ring-1 ring-cyan-300/35'
				: 'border-red-300/55 bg-red-500/10 ring-1 ring-red-300/35';
		}
		return 'border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]';
	}

	function captainName(team: AuctionTournamentTeam): string {
		return (
			team.members.find((member) => member.acquiredVia === 'CAPTAIN')?.displayName ?? '팀장 미지정'
		);
	}

	function acquisitionLabel(member: AuctionTournamentTeam['members'][number]): string {
		if (member.acquiredVia === 'CAPTAIN') return '팀장';
		if (member.acquiredVia === 'MANUAL') return '수동';
		return '낙찰';
	}

	function acquisitionBadgeClass(member: AuctionTournamentTeam['members'][number]): string {
		if (member.acquiredVia === 'MANUAL') return 'bg-cyan-400/10 text-cyan-100';
		if (member.acquiredVia === 'CAPTAIN') return 'bg-amber-300/10 text-amber-100';
		return 'bg-slate-950/35 text-slate-100';
	}
</script>

<div class="grid gap-2">
	<section class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
		<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-2 xl:grid-cols-4">
			<div class="bg-[var(--color-app-surface)] px-3 py-3">
				<p class="text-xs text-slate-500">입찰 풀</p>
				<p class="mt-1 text-2xl font-semibold text-slate-100 tabular-nums">{candidatePoolSize}</p>
			</div>
			<div class="bg-[var(--color-app-surface)] px-3 py-3">
				<p class="text-xs text-slate-500">배치</p>
				<p class="mt-1 text-2xl font-semibold text-emerald-100 tabular-nums">
					{assignedCount}/{candidatePoolSize}
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
					style={`width: ${expectedTotal > 0 ? Math.min(100, (placedCount / expectedTotal) * 100) : 0}%`}
				></div>
			</div>
		</div>
	</section>

	<div class="grid gap-2 xl:grid-cols-[minmax(24rem,34rem)_minmax(0,1fr)]">
		<div class="grid gap-2 xl:h-[56rem] xl:grid-rows-[minmax(0,8fr)_minmax(0,2fr)]">
			<section
				class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] border-y border-cyan-400/30 bg-[var(--color-app-surface)]"
			>
				<div
					class="flex items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
				>
					<div class="flex items-center gap-2">
						<p class="text-base font-semibold text-slate-100">현재 매물</p>
						{#if currentTarget}
							<span class="size-2 rounded-full bg-emerald-300"></span>
						{/if}
					</div>
					<div class="flex flex-wrap items-center justify-end gap-1.5">
						{#if canRevert}
							<button
								type="button"
								class="inline-flex h-9 items-center gap-1.5 rounded-sm px-3 text-sm font-semibold text-amber-100 hover:bg-amber-400/10"
								onclick={openRevertDialog}
							>
								<Undo2 size={14} aria-hidden="true" />
								{revertLabel}
							</button>
						{/if}
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
							<Dice5 size={14} aria-hidden="true" /> 다음 후보
						</button>
					</div>
				</div>

				<div class="grid min-h-0 grid-rows-[minmax(0,1fr)_auto] gap-3 overflow-hidden p-3">
					{#if currentTarget}
						{@const role = currentTargetRole()}
						{@const profile = currentTargetProfile}
						<div
							class={showTargetDetails
								? 'grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-3 overflow-hidden'
								: 'grid min-h-0 content-start gap-3 overflow-y-auto pr-1'}
						>
							{#if showTargetDetails}
								<div
									class={`cursor-grab border px-4 py-4 active:cursor-grabbing ${
										draggedTargetId === currentTarget.userId
											? 'border-cyan-300/60 bg-cyan-400/15 opacity-70'
											: 'border-cyan-400/30 bg-cyan-400/5'
									}`}
									draggable={canManageDraft}
									ondragstart={startTargetDrag}
									ondragend={clearDragState}
									role="group"
									aria-label={`현재 경매 매물 ${currentTarget.displayName}`}
								>
									<div class="flex items-start gap-4">
										<PlayerAvatar
											name={currentTarget.displayName}
											{role}
											avatarUrl={profile?.avatarUrl}
											profileIconId={profile?.profileIconId}
											profileIconVersion={championCatalog?.version}
											size="md"
										/>
										<div class="min-w-0 flex-1">
											<p class="truncate text-2xl font-semibold text-slate-50">
												{currentTarget.displayName}
											</p>
											<div class="mt-2 flex flex-wrap items-center gap-1.5">
												<span
													class="inline-flex items-center gap-1 border border-cyan-400/25 bg-cyan-400/10 px-2 py-1 text-sm font-semibold text-cyan-100"
												>
													<PositionIcon {role} size="sm" />
													{playerLaneLabel(currentTarget)}
												</span>
												{#if profile?.riotTier}
													<span
														class="border border-amber-300/25 bg-amber-300/10 px-2 py-1 text-sm font-semibold text-amber-100"
													>
														{profile.riotTier}
													</span>
												{/if}
												<span class="px-2 py-1 text-sm font-semibold text-slate-300">
													MMR {mmrLabel(currentTarget.mmrAtStart)}
												</span>
											</div>
										</div>
									</div>
								</div>

								{#if profile}
									<div
										class="grid min-h-0 content-start gap-4 overflow-y-auto overscroll-contain bg-[var(--color-app-bg-soft)] p-3 pr-[13px]"
									>
										<section class="grid gap-3">
											<div class="flex items-center justify-between gap-3">
												<div class="flex items-center gap-2">
													<Trophy size={16} class="text-cyan-100" aria-hidden="true" />
													<p class="text-sm font-semibold text-slate-100">내전 전적</p>
												</div>
												<p class="text-xs font-semibold text-slate-500">
													{profile.summary.wins}승 {profile.summary.losses}패
												</p>
											</div>
											<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
												<div class="bg-[var(--color-app-surface)] p-2">
													<p class="text-[11px] text-slate-500">내전</p>
													<p class="mt-1 text-sm font-semibold text-slate-100">
														{profile.summary.games}게임
													</p>
												</div>
												<div class="bg-[var(--color-app-surface)] p-2">
													<p class="text-[11px] text-slate-500">승률</p>
													<p class="mt-1 text-sm font-semibold text-cyan-100">{recordWinRate()}</p>
												</div>
												<div class="bg-[var(--color-app-surface)] p-2">
													<p class="text-[11px] text-slate-500">주력 포지션</p>
													<p class="mt-1 text-sm font-semibold text-slate-100">
														{laneLabels[profile.mainLane ?? role]}
													</p>
												</div>
											</div>
										</section>

										<section class="grid gap-2">
											<p class="text-xs font-semibold text-slate-400">내전 주력 챔피언</p>
											{#if profile.championRecords.length > 0}
												<div class="grid gap-px bg-[var(--color-app-border-muted)]">
													{#each profile.championRecords as record (record.championId)}
														{@const champion = championFor(record.championId)}
														<div
															class="grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 bg-[var(--color-app-surface)] p-2"
														>
															{#if champion}
																<ChampionFace
																	{champion}
																	version={championCatalog?.version ?? ''}
																	size="sm"
																/>
															{:else}
																<span
																	class="grid size-8 place-items-center bg-slate-950/45 text-[10px] font-semibold text-slate-400"
																>
																	{record.championId.slice(0, 1).toUpperCase()}
																</span>
															{/if}
															<div class="min-w-0">
																<p class="truncate text-sm font-semibold text-slate-100">
																	{champion?.name ?? record.championId}
																</p>
																<p class="text-[11px] font-medium text-slate-500">
																	{record.games}게임 · {record.wins}승 {record.losses}패
																</p>
															</div>
															<p class="text-xs font-semibold text-cyan-100">
																{winRate(record.games, record.wins)}
															</p>
														</div>
													{/each}
												</div>
											{:else}
												<p class="bg-[var(--color-app-surface)] px-3 py-2 text-sm text-slate-500">
													확정된 내전 전적 없음
												</p>
											{/if}
										</section>

										<section class="grid gap-2">
											<p class="text-xs font-semibold text-slate-400">라인별 MMR</p>
											<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-5">
												{#each profile.laneMmr as lane (lane.lane)}
													<div class="bg-[var(--color-app-surface)] p-2">
														<div class="flex items-center justify-between gap-2">
															<span class="text-xs font-semibold text-slate-400"
																>{laneLabels[lane.lane]}</span
															>
															<span class="text-[11px] font-semibold text-slate-500"
																>{lane.games}게임</span
															>
														</div>
														<p class="mt-1 text-sm font-semibold text-slate-100 tabular-nums">
															{mmrLabel(lane.mmr)}
														</p>
													</div>
												{/each}
											</div>
										</section>

										<section class="grid gap-2">
											<p class="text-xs font-semibold text-slate-400">선호 챔피언</p>
											{#if profile.championIds.length > 0}
												<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-3">
													{#each profile.championIds.slice(0, 3) as championId (championId)}
														{@const champion = championFor(championId)}
														<div
															class="flex min-w-0 items-center gap-2 bg-[var(--color-app-surface)] p-2"
														>
															{#if champion}
																<ChampionFace
																	{champion}
																	version={championCatalog?.version ?? ''}
																	size="sm"
																/>
																<span class="min-w-0 truncate text-sm font-semibold text-slate-100"
																	>{champion.name}</span
																>
															{:else}
																<span class="min-w-0 truncate text-sm font-semibold text-slate-300"
																	>{championId}</span
																>
															{/if}
														</div>
													{/each}
												</div>
											{:else}
												<p class="bg-[var(--color-app-surface)] px-3 py-2 text-sm text-slate-500">
													등록한 선호 챔피언 없음
												</p>
											{/if}
										</section>

										<section class="grid gap-2">
											<div class="flex items-center justify-between gap-3">
												<p class="text-xs font-semibold text-slate-400">최근 게임</p>
												<p class="text-xs font-semibold text-slate-500">{recentRecordSummary()}</p>
											</div>
											{#if profile.recentGames.length > 0}
												<div
													class="flex gap-0.5"
													aria-label={`최근 ${profile.recentGames.length}게임 ${recentRecordSummary()}`}
												>
													{#each profile.recentGames as game (game.id)}
														<span
															class={`h-1.5 min-w-0 flex-1 ${
																game.result === 'WIN' ? 'bg-cyan-300/80' : 'bg-rose-300/70'
															}`}
														></span>
													{/each}
												</div>
												<div class="grid max-h-44 gap-1 overflow-y-auto pr-[5px]">
													{#each profile.recentGames as game (game.id)}
														{@const champion = game.championId
															? championFor(game.championId)
															: null}
														<div
															class="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 bg-[var(--color-app-surface)] p-2"
														>
															{#if champion}
																<ChampionFace
																	{champion}
																	version={championCatalog?.version ?? ''}
																	size="sm"
																/>
															{:else}
																<span
																	class="grid size-8 place-items-center bg-slate-950/45 text-[10px] font-semibold text-slate-400"
																>
																	{game.championId?.slice(0, 1).toUpperCase() ??
																		laneLabels[game.lane].slice(0, 1)}
																</span>
															{/if}
															<div class="min-w-0">
																<p class="truncate text-xs font-semibold text-slate-100">
																	{champion?.name ?? game.championId ?? '챔피언 미입력'}
																</p>
																<p class="truncate text-[11px] font-medium text-slate-500">
																	{laneLabels[game.lane]} · {playedAtLabel(game.playedAt)}
																</p>
															</div>
															<span
																class={`text-xs font-semibold ${game.result === 'WIN' ? 'text-cyan-100' : 'text-rose-200'}`}
															>
																{game.result === 'WIN' ? '승' : '패'}
															</span>
														</div>
													{/each}
												</div>
											{:else}
												<p class="bg-[var(--color-app-surface)] px-3 py-2 text-sm text-slate-500">
													확정된 최근 게임 없음
												</p>
											{/if}
										</section>

										<section class="grid gap-3">
											<div class="flex items-center justify-between gap-3">
												<div class="flex items-center gap-2">
													<Swords size={16} class="text-amber-100" aria-hidden="true" />
													<p class="text-sm font-semibold text-slate-100">Riot 전적</p>
												</div>
												{#if profile.riot}
													<p class="text-xs font-semibold text-slate-500">
														{playedAtLabel(profile.riot.refreshedAt)}
													</p>
												{/if}
											</div>
											{#if profile.riot}
												<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
													<div class="bg-[var(--color-app-surface)] p-2">
														<p class="text-[11px] text-slate-500">솔로 랭크</p>
														<p class="mt-1 text-sm font-semibold text-amber-100">
															{profile.riotTier ?? '-'}
														</p>
													</div>
													<div class="bg-[var(--color-app-surface)] p-2">
														<p class="text-[11px] text-slate-500">최근</p>
														<p class="mt-1 text-sm font-semibold text-slate-100">
															{profile.riot.games}게임
														</p>
													</div>
													<div class="bg-[var(--color-app-surface)] p-2">
														<p class="text-[11px] text-slate-500">승률</p>
														<p class="mt-1 text-sm font-semibold text-amber-100">
															{winRate(profile.riot.games, profile.riot.wins)}
														</p>
													</div>
												</div>
												{#if profile.riot.championIds.length > 0}
													<div class="grid gap-px bg-[var(--color-app-border-muted)]">
														{#each profile.riot.championIds as championId (championId)}
															{@const champion = championFor(championId)}
															<div
																class="flex min-w-0 items-center gap-2 bg-[var(--color-app-surface)] p-2"
															>
																{#if champion}
																	<ChampionFace
																		{champion}
																		version={championCatalog?.version ?? ''}
																		size="sm"
																	/>
																{/if}
																<span class="min-w-0 truncate text-sm font-semibold text-slate-100">
																	{champion?.name ?? championId}
																</span>
															</div>
														{/each}
													</div>
												{/if}
											{:else}
												<p class="bg-[var(--color-app-surface)] px-3 py-2 text-sm text-slate-500">
													캐시된 Riot 전적 없음
												</p>
											{/if}
										</section>
									</div>
								{/if}
							{:else}
								<div
									class={`relative isolate grid min-h-[30rem] cursor-grab overflow-hidden border px-4 py-5 active:cursor-grabbing xl:min-h-[32rem] ${
										draggedTargetId === currentTarget.userId
											? 'border-cyan-300/60 bg-cyan-400/15 opacity-70'
											: 'border-cyan-400/30 bg-slate-950/35'
									}`}
									draggable={canManageDraft}
									ondragstart={startTargetDrag}
									ondragend={clearDragState}
									role="group"
									aria-label={`현재 경매 매물 ${currentTarget.displayName}`}
								>
									<div class="pointer-events-none absolute right-5 top-5 opacity-20">
										<PositionIcon {role} size="md" />
									</div>
									<div
										class="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-cyan-400/[0.045]"
									></div>
									<div class="z-10 grid h-full content-between gap-5">
										<div class="flex items-start justify-between gap-3">
											<span
												class="inline-flex items-center gap-1 border border-cyan-400/25 bg-cyan-400/10 px-2 py-1 text-sm font-semibold text-cyan-100"
											>
												<PositionIcon {role} size="sm" />
												{playerLaneLabel(currentTarget)}
											</span>
											{#if profile?.riotTier}
												<span
													class="border border-amber-300/25 bg-amber-300/10 px-2 py-1 text-sm font-semibold text-amber-100"
												>
													{profile.riotTier}
												</span>
											{/if}
										</div>

										<div class="grid justify-items-center gap-3 text-center">
											<PlayerAvatar
												name={currentTarget.displayName}
												{role}
												avatarUrl={profile?.avatarUrl}
												profileIconId={profile?.profileIconId}
												profileIconVersion={championCatalog?.version}
												size="md"
											/>
											<div class="min-w-0">
												<p class="max-w-72 truncate text-3xl font-semibold text-slate-50">
													{currentTarget.displayName}
												</p>
												<p class="mt-1 text-base font-semibold text-slate-300">
													MMR {mmrLabel(currentTarget.mmrAtStart)}
												</p>
											</div>
										</div>

										{#if profile}
											<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
												<div class="bg-slate-950/70 px-3 py-2 text-center">
													<p class="text-[11px] text-slate-500">승률</p>
													<p class="mt-1 text-base font-semibold text-cyan-100">
														{recordWinRate()}
													</p>
												</div>
												<div class="bg-slate-950/70 px-3 py-2 text-center">
													<p class="text-[11px] text-slate-500">승패</p>
													<p class="mt-1 text-base font-semibold text-slate-100">
														{profile.summary.wins}-{profile.summary.losses}
													</p>
												</div>
												<div class="bg-slate-950/70 px-3 py-2 text-center">
													<p class="text-[11px] text-slate-500">최근</p>
													<p class="mt-1 text-base font-semibold text-slate-100">
														{recentRecordSummary()}
													</p>
												</div>
											</div>
										{/if}
									</div>
								</div>
							{/if}
						</div>
						<div class="flex justify-end">
							<button
								type="button"
								class="inline-flex h-10 items-center gap-1.5 rounded-sm px-3.5 text-sm font-semibold text-amber-100 hover:bg-amber-300/10 disabled:cursor-not-allowed disabled:opacity-40"
								disabled={!canManageDraft}
								onclick={markUnsold}
							>
								<CircleSlash size={14} aria-hidden="true" /> 유찰
							</button>
						</div>
					{:else if allPlaced}
						<div class="grid min-h-80 place-items-center text-center">
							<div>
								<Gavel class="mx-auto text-emerald-200" size={28} aria-hidden="true" />
								<p class="mt-3 text-base font-semibold text-emerald-100">브래킷 시작 가능</p>
								<p class="mt-1 text-sm text-slate-500">모든 팀 정원이 찼습니다.</p>
							</div>
						</div>
					{:else}
						<div
							class="grid min-h-80 place-items-center border border-dashed border-slate-700 text-center text-base text-slate-500"
						>
							<div>
								<PackageOpen class="mx-auto" size={24} aria-hidden="true" />
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
					class="flex items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3 py-2"
				>
					<p class="text-base font-semibold text-slate-100">유찰</p>
					<p class="text-xs text-slate-500">팀 배치 또는 재등록</p>
				</div>
				<div role="list" class="grid min-h-0 content-start gap-1.5 overflow-y-auto p-2">
					{#each unsold as member (member.userId)}
						<div
							role="listitem"
							draggable={canManageDraft}
							ondragstart={(event) => startUnsoldDrag(event, member)}
							ondragend={clearDragState}
							class={`grid min-w-0 gap-2 border border-amber-300/10 bg-amber-300/[0.045] p-2 text-left text-sm text-slate-100 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center ${
								draggedTargetId === member.userId ? 'opacity-60' : ''
							}`}
							aria-label={`${member.displayName} 유찰 후보`}
						>
							<div class="flex min-w-0 items-center gap-2">
								<span
									class="grid size-8 shrink-0 place-items-center border border-amber-300/20 bg-slate-950/70 text-xs font-semibold text-amber-100"
								>
									{playerInitial(member.displayName)}
								</span>
								<span class="min-w-0 flex-1">
									<span class="block truncate text-sm font-semibold">{member.displayName}</span>
									<span class="block truncate text-[11px] text-slate-500">
										{playerLaneLabel(member)} · MMR {mmrLabel(member.mmrAtStart)}
									</span>
								</span>
							</div>
							<div class="flex justify-end gap-1">
								<button
									type="button"
									class="h-8 px-2 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10 disabled:cursor-not-allowed disabled:opacity-40"
									disabled={!canManageDraft || Boolean(currentTarget)}
									onclick={() => restoreUnsoldToTarget(member.userId)}
								>
									재등록
								</button>
							</div>
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
					aria-label={`${team.teamName} 입찰 팀`}
				>
					<div class="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-2">
						<div class="min-w-0">
							<div class="flex min-w-0 items-center gap-1.5">
								<span
									class="shrink-0 border border-cyan-400/25 px-1.5 py-0.5 text-xs font-semibold text-cyan-100"
								>
									{team.teamName}
								</span>
								<p class="min-w-0 truncate text-base font-semibold text-slate-100">
									{captainName(team)}
								</p>
							</div>
							<p class="mt-1 text-xs text-slate-500">시작 {team.initialPoints}P</p>
						</div>
						<div
							class="shrink-0 border border-amber-300/30 px-2.5 py-1 text-base font-bold text-amber-100 tabular-nums"
						>
							{team.currentPoints}P
						</div>
					</div>

					<div class="mt-2 grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-3">
						<div class="grid gap-2">
							<div class="flex items-center justify-between text-xs text-slate-500">
								<span>인원 {team.members.length}/5</span>
							</div>
							<div class="grid grid-cols-5 gap-1">
								{#each teamMemberSlots as slot (slot)}
									<span class={`h-2 border ${memberSlotClass(team, slot)}`}></span>
								{/each}
							</div>

							<div class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-1">
								<input
									type="text"
									inputmode="numeric"
									pattern="[0-9]*"
									class="h-9 min-w-0 border border-[var(--color-app-border-muted)] bg-slate-950/35 px-3 text-right text-base font-semibold text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-300/50"
									placeholder="입찰가"
									disabled={!canManageDraft || !currentTarget || team.members.length >= 5}
									value={bidInputs[team.id] ?? ''}
									oninput={(event) => {
										bidInputs = { ...bidInputs, [team.id]: event.currentTarget.value };
									}}
									aria-label={`${team.teamName} 입찰가`}
								/>
								<button
									type="button"
									class="inline-flex h-9 items-center border border-cyan-400/25 px-2.5 text-sm font-bold text-cyan-100 hover:border-cyan-300/45 disabled:cursor-not-allowed disabled:opacity-40"
									disabled={!canManageDraft || !currentTarget || team.members.length >= 5}
									title="좌클릭 +10 / 우클릭 -10"
									aria-label={`${team.teamName} 입찰가 10점 조절`}
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
									onclick={() => finalizeBid(team)}
								>
									<Gavel size={14} aria-hidden="true" /> 낙찰
								</button>
							</div>
						</div>

						<div
							role="list"
							class="grid min-h-0 content-start gap-1 overflow-x-hidden overflow-y-auto overscroll-contain"
							aria-label={`${team.teamName} 낙찰 멤버`}
						>
							{#each team.members as member (member.teamMemberId)}
								<div
									role="listitem"
									class="flex min-h-11 items-center gap-2.5 bg-slate-950/20 px-2 py-1.5 text-sm"
								>
									<span
										class="grid size-7 shrink-0 place-items-center border border-slate-600 bg-slate-950/70 text-[11px] font-semibold text-slate-200"
									>
										{playerInitial(member.displayName)}
									</span>
									<div class="min-w-0 flex-1">
										<p class="truncate text-base font-semibold text-slate-100">
											{member.displayName}
										</p>
										<p class="mt-0.5 truncate text-xs font-medium text-slate-500">
											{playerLaneLabel(member)} · MMR {mmrLabel(member.mmrAtAcquisition)}
										</p>
									</div>
									<span
										class={`shrink-0 px-2 py-0.5 text-xs font-semibold ${acquisitionBadgeClass(member)}`}
									>
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

<Dialog.Root bind:open={revertDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex items-center justify-between gap-3">
				<Dialog.Title class="text-sm font-semibold text-slate-50">{revertLabel}</Dialog.Title>
				<Dialog.Close
					disabled={submitting}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="입찰 되돌리기 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</div>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				{#if snapshot.tournament.status === 'BRACKET_SETUP'}
					마지막 입찰 작업과 해당 선수 배치·포인트를 되돌리고, 토너먼트를 입찰 단계로 복구합니다.
				{:else}
					가장 최근 입찰 작업과 해당 선수 배치·포인트를 함께 되돌립니다.
				{/if}
			</Dialog.Description>
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={submitting}
					class="h-8 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={!canRevert}
					onclick={() => void revertLastAction()}
					class="inline-flex h-8 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					<Undo2 size={14} aria-hidden="true" />
					{revertLabel}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
