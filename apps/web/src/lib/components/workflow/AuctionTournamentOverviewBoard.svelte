<script lang="ts">
	import { Medal, ReceiptText, Swords, Trophy } from 'lucide-svelte';
	import type {
		AuctionTournamentResultGame,
		AuctionTournamentResultLineup,
		AuctionTournamentResultMatch,
		AuctionTournamentResultPlayer,
		AuctionTournamentResultSnapshot,
		AuctionTournamentResultTeam,
		AuctionTournamentResultTeamMember
	} from '$lib/contracts/auctionTournamentResult';
	import { formatRoundedNumber, formatSignedMmr } from '$lib/numberFormat';
	import PositionIcon from './PositionIcon.svelte';

	type MatchSide = 'LEFT' | 'RIGHT';
	type GameSide = 'BLUE' | 'RED';
	type Lane = 'TOP' | 'JUNGLE' | 'MID' | 'BOTTOM' | 'SUPPORT';
	type MatchLineupRow = {
		lineup: AuctionTournamentResultLineup;
		member: AuctionTournamentResultTeamMember;
	};

	interface Props {
		snapshot: AuctionTournamentResultSnapshot;
	}

	let { snapshot }: Props = $props();

	const lanes: readonly Lane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const laneLabels: Record<Lane, string> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '바텀',
		SUPPORT: '서포트'
	};
	const matchSides: readonly MatchSide[] = ['LEFT', 'RIGHT'];
	const gameSides: readonly GameSide[] = ['BLUE', 'RED'];
	const finalMatch = $derived(snapshot.matches.find((match) => match.matchKey === 'FINAL') ?? null);
	const championTeam = $derived(teamById(snapshot.tournament.championTeamId));
	const orderedMatches = $derived(
		snapshot.matches.slice().sort((left, right) => matchOrder(left) - matchOrder(right))
	);
	const orderedTeams = $derived(
		snapshot.teams.slice().sort((left, right) => left.teamIndex - right.teamIndex)
	);
	const playerRows = $derived(
		snapshot.players.slice().sort((left, right) => {
			const leftTeam = teamById(left.teamId)?.teamIndex ?? Number.MAX_SAFE_INTEGER;
			const rightTeam = teamById(right.teamId)?.teamIndex ?? Number.MAX_SAFE_INTEGER;
			if (leftTeam !== rightTeam) return leftTeam - rightTeam;
			return left.displayName.localeCompare(right.displayName, 'ko-KR');
		})
	);

	function matchOrder(match: AuctionTournamentResultMatch): number {
		if (match.matchKey === 'SEMIFINAL_A') return 0;
		if (match.matchKey === 'SEMIFINAL_B') return 1;
		return 2;
	}

	function teamById(teamId: string | null): AuctionTournamentResultTeam | null {
		return teamId ? (snapshot.teams.find((team) => team.id === teamId) ?? null) : null;
	}

	function captainName(team: AuctionTournamentResultTeam): string {
		return (
			team.members.find((member) => member.acquiredVia === 'CAPTAIN')?.displayName ?? '팀장 미지정'
		);
	}

	function teamMembers(team: AuctionTournamentResultTeam): AuctionTournamentResultTeamMember[] {
		return team.members.slice().sort((left, right) => {
			const leftLane = left.declaredLane
				? lanes.indexOf(left.declaredLane)
				: Number.MAX_SAFE_INTEGER;
			const rightLane = right.declaredLane
				? lanes.indexOf(right.declaredLane)
				: Number.MAX_SAFE_INTEGER;
			if (leftLane !== rightLane) return leftLane - rightLane;
			return left.displayName.localeCompare(right.displayName, 'ko-KR');
		});
	}

	function memberInitial(displayName: string): string {
		return displayName.trim().slice(0, 1).toLocaleUpperCase('ko-KR') || '?';
	}

	function laneLabel(lane: Lane | null): string {
		return lane ? laneLabels[lane] : '라인 미설정';
	}

	function declaredLaneLabel(lane: Lane | null): string {
		return lane ? `희망 ${laneLabel(lane)}` : '희망 라인 미설정';
	}

	function memberAcquisitionLabel(member: AuctionTournamentResultTeamMember): string {
		if (member.acquiredVia === 'CAPTAIN') return '팀장';
		if (member.acquiredVia === 'MANUAL') return '수동 배치';
		return member.acquisitionPoints === null
			? '낙찰 포인트 미기록'
			: pointLabel(member.acquisitionPoints);
	}

	function pointLabel(points: number): string {
		return `${formatRoundedNumber(points)}P`;
	}

	function teamFor(
		match: AuctionTournamentResultMatch,
		side: MatchSide
	): AuctionTournamentResultTeam | null {
		return teamById(side === 'LEFT' ? match.leftTeamId : match.rightTeamId);
	}

	function matchLineupRows(match: AuctionTournamentResultMatch, side: MatchSide): MatchLineupRow[] {
		const team = teamFor(match, side);
		if (!team) return [];

		return match.lineups
			.filter((lineup) => lineup.teamId === team.id)
			.flatMap((lineup) => {
				const member = team.members.find(
					(candidate) =>
						candidate.teamMemberId === lineup.teamMemberId && candidate.userId === lineup.userId
				);
				return member ? [{ lineup, member }] : [];
			})
			.sort((left, right) => lanes.indexOf(left.lineup.lane) - lanes.indexOf(right.lineup.lane));
	}

	function matchSideForGameSide(game: AuctionTournamentResultGame, side: GameSide): MatchSide {
		const leftTeamSide: GameSide = game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
		return side === leftTeamSide ? 'LEFT' : 'RIGHT';
	}

	function gameTeamForSide(
		match: AuctionTournamentResultMatch,
		game: AuctionTournamentResultGame,
		side: GameSide
	): AuctionTournamentResultTeam | null {
		return teamFor(match, matchSideForGameSide(game, side));
	}

	function gameLineupRows(
		match: AuctionTournamentResultMatch,
		game: AuctionTournamentResultGame,
		side: GameSide
	): MatchLineupRow[] {
		return matchLineupRows(match, matchSideForGameSide(game, side));
	}

	function gameWinnerLabel(
		match: AuctionTournamentResultMatch,
		game: AuctionTournamentResultGame
	): string {
		const team = gameTeamForSide(match, game, game.winner);
		return team ? `${team.teamName} 승리` : `${game.winner} 승리`;
	}

	function championLabel(championId: string | null): string {
		return championId ?? '미입력';
	}

	function bansLabel(championIds: readonly (string | null)[]): string {
		const selected = championIds.filter((championId): championId is string => championId !== null);
		return selected.length > 0 ? selected.join(' · ') : '없음';
	}

	function scoreLabel(match: AuctionTournamentResultMatch, side: MatchSide): string {
		if (!teamFor(match, side)) return '-';
		return String(side === 'LEFT' ? match.leftScore : match.rightScore);
	}

	function matchSideClass(
		match: AuctionTournamentResultMatch,
		team: AuctionTournamentResultTeam | null
	): string {
		if (!team) return 'border-transparent bg-slate-950/35 text-slate-600';
		if (match.winnerTeamId === team.id) {
			return 'border-emerald-300/35 bg-emerald-400/10 text-emerald-50';
		}
		return 'border-transparent bg-slate-950/35 text-slate-300';
	}

	function matchLabel(match: AuctionTournamentResultMatch): string {
		if (match.matchKey === 'SEMIFINAL_A') return '준결승 A';
		if (match.matchKey === 'SEMIFINAL_B') return '준결승 B';
		return '결승';
	}

	function matchStatusLabel(match: AuctionTournamentResultMatch): string {
		if (match.status === 'LINEUP_PENDING') return '라인업 대기';
		if (match.status === 'READY') return '시작 준비';
		if (match.status === 'IN_PROGRESS') return '진행 중';
		if (match.status === 'COMPLETED') return match.endedEarly ? '조기 마무리' : '완료';
		return '취소됨';
	}

	function tournamentStateLabel(): string {
		if (championTeam) return `${championTeam.teamName} 우승`;
		if (snapshot.tournament.status === 'CANCELLED') return '토너먼트 취소됨';
		if (snapshot.tournament.status === 'COMPLETED') return '우승팀 없이 마무리';
		if (!finalMatch) return '대진 생성 대기';
		return finalMatch.status === 'COMPLETED' ? '우승팀 확정 대기' : matchStatusLabel(finalMatch);
	}

	function teamPanelClass(team: AuctionTournamentResultTeam): string {
		if (team.id === snapshot.tournament.championTeamId) {
			return 'border-amber-300/40 bg-amber-300/[0.075]';
		}
		return 'border-[var(--color-app-border-muted)] bg-slate-950/20';
	}

	function playerLanes(player: AuctionTournamentResultPlayer): Lane[] {
		return [
			...new Set(
				snapshot.matches.flatMap((match) =>
					match.lineups
						.filter((lineup) => lineup.userId === player.userId)
						.map((lineup) => lineup.lane)
				)
			)
		].sort((left, right) => lanes.indexOf(left) - lanes.indexOf(right));
	}

	function playerLaneLabel(player: AuctionTournamentResultPlayer): string {
		const assignedLanes = playerLanes(player);
		if (assignedLanes.length === 0) return '라인 미확정';
		if (assignedLanes.length === 1) return laneLabel(assignedLanes[0] ?? null);
		return assignedLanes.map((lane) => laneLabel(lane)).join(' · ');
	}

	function playerPrimaryLane(player: AuctionTournamentResultPlayer): Lane | null {
		const assignedLanes = playerLanes(player);
		return assignedLanes.length === 1 ? (assignedLanes[0] ?? null) : null;
	}

	function mmrDeltaLabel(delta: number | null): string {
		return delta === null ? '미반영' : formatSignedMmr(delta);
	}

	function mmrDeltaClass(delta: number | null): string {
		if (delta === null) return 'text-slate-500';
		if (delta > 0) return 'text-emerald-300';
		if (delta < 0) return 'text-rose-300';
		return 'text-slate-400';
	}
</script>

<div class="grid gap-2 pb-2">
	<div class="grid gap-2 xl:grid-cols-[minmax(20rem,28rem)_minmax(0,1fr)]">
		<section
			class="grid grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] xl:h-[min(40rem,calc(100dvh-16rem))] xl:min-h-[30rem]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<div class="flex items-center justify-between gap-3">
					<div class="min-w-0">
						<p class="text-base font-semibold text-slate-100">대진 결과</p>
						<p class="mt-1 truncate text-xs text-slate-500">{tournamentStateLabel()}</p>
					</div>
					<Swords size={18} class="text-cyan-200" aria-hidden="true" />
				</div>
			</div>

			<div
				class="grid min-h-0 content-start gap-2 p-3 pr-[13px] xl:overflow-x-hidden xl:overflow-y-auto xl:overscroll-contain"
			>
				{#each orderedMatches as match (match.id)}
					<section class="grid gap-2 bg-slate-950/20 p-3">
						<div class="flex items-center justify-between gap-3">
							<div class="flex min-w-0 items-center gap-2">
								<p class="truncate text-sm font-semibold text-cyan-100">{matchLabel(match)}</p>
								<span class="shrink-0 text-xs font-semibold text-slate-500">BO{match.bestOf}</span>
							</div>
							<span class="shrink-0 text-xs font-semibold text-slate-500"
								>{matchStatusLabel(match)}</span
							>
						</div>

						<div class="grid grid-cols-[minmax(0,1fr)_3.25rem_minmax(0,1fr)] items-stretch gap-2">
							{#each matchSides as side (side)}
								{@const team = teamFor(match, side)}
								<div class={`grid min-h-20 border px-3 py-2 ${matchSideClass(match, team)}`}>
									<div class="flex min-w-0 items-center justify-between gap-3">
										<div class="min-w-0">
											<p class="truncate text-lg font-semibold">{team?.teamName ?? '팀 대기'}</p>
											<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
												{team ? `${captainName(team)} 팀장` : '상대 확정 대기'}
											</p>
										</div>
										<p class="font-mono text-3xl font-black tabular-nums">
											{scoreLabel(match, side)}
										</p>
									</div>
								</div>

								{#if side === 'LEFT'}
									<div class="grid place-items-center text-sm font-black text-slate-500">VS</div>
								{/if}
							{/each}
						</div>

						{#if match.hardFearless}
							<p class="text-xs font-medium text-amber-100">하드 피어리스</p>
						{/if}

						{#if match.lineups.length > 0 || match.games.length > 0}
							<details class="border border-[var(--color-app-border-muted)] bg-slate-950/15">
								<summary
									class="flex cursor-pointer items-center justify-between gap-3 px-3 py-2 text-xs font-semibold text-slate-300"
								>
									<span>확정 라인업 · 세트 상세</span>
									<span class="shrink-0 text-slate-500"
										>{match.lineups.length}명 · {match.games.length}세트</span
									>
								</summary>

								<div class="grid gap-3 border-t border-[var(--color-app-border-muted)] p-3">
									{#if match.lineups.length > 0}
										<section class="grid gap-1.5">
											<p class="text-[11px] font-bold uppercase text-slate-500">확정 라인업</p>
											<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-2">
												{#each matchSides as side (side)}
													{@const team = teamFor(match, side)}
													<div class="grid gap-px bg-[var(--color-app-surface)] p-2">
														<p class="truncate px-1 text-xs font-semibold text-slate-200">
															{team?.teamName ?? '팀 대기'}
														</p>
														{#each matchLineupRows(match, side) as row (row.lineup.teamMemberId)}
															<div
																class="grid min-h-8 grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-2 bg-slate-950/25 px-2"
															>
																<PositionIcon role={row.lineup.lane} size="sm" />
																<span class="truncate text-xs font-semibold text-slate-300">
																	{row.member.displayName}
																</span>
															</div>
														{:else}
															<p class="px-1 py-2 text-xs font-semibold text-slate-600">
																라인업 대기
															</p>
														{/each}
													</div>
												{/each}
											</div>
										</section>
									{/if}

									{#if match.games.length > 0}
										<section class="grid gap-1.5">
											<p class="text-[11px] font-bold uppercase text-slate-500">확정 세트</p>
											<div class="grid gap-2">
												{#each match.games
													.slice()
													.sort((left, right) => left.setNumber - right.setNumber) as game (game.id)}
													<section
														class="grid gap-2 border border-[var(--color-app-border-muted)] bg-slate-950/20 p-2"
													>
														<div class="flex min-w-0 items-center justify-between gap-3">
															<p class="truncate text-xs font-semibold text-slate-200">
																{game.setNumber}세트 · {gameWinnerLabel(match, game)}
															</p>
															<span class="shrink-0 text-[11px] font-semibold text-slate-500">
																{game.sidePreset === 'DEFAULT' ? '왼쪽 팀 블루' : '오른쪽 팀 블루'}
															</span>
														</div>

														<div
															class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-2"
														>
															{#each gameSides as side (side)}
																{@const team = gameTeamForSide(match, game, side)}
																<div class="grid gap-px bg-[var(--color-app-surface)] p-2">
																	<div class="flex min-w-0 items-center justify-between gap-2 px-1">
																		<p
																			class={`truncate text-xs font-semibold ${side === 'BLUE' ? 'text-cyan-200' : 'text-rose-200'}`}
																		>
																			{side} · {team?.teamName ?? '팀 대기'}
																		</p>
																		{#if game.winner === side}
																			<span class="text-[10px] font-bold text-emerald-300"
																				>승리</span
																			>
																		{/if}
																	</div>
																	<p class="truncate px-1 text-[11px] font-medium text-slate-500">
																		밴 · {bansLabel(game.bans[side])}
																	</p>
																	{#each gameLineupRows(match, game, side) as row (row.lineup.teamMemberId)}
																		<div
																			class="grid min-h-8 grid-cols-[1.25rem_minmax(0,1fr)_minmax(0,0.85fr)] items-center gap-2 bg-slate-950/25 px-2"
																		>
																			<PositionIcon role={row.lineup.lane} size="sm" />
																			<span class="truncate text-xs font-semibold text-slate-300">
																				{row.member.displayName}
																			</span>
																			<span
																				class="truncate text-right font-mono text-[11px] font-semibold text-amber-100"
																			>
																				{championLabel(game.picks[row.lineup.userId] ?? null)}
																			</span>
																		</div>
																	{:else}
																		<p class="px-1 py-2 text-xs font-semibold text-slate-600">
																			라인업 기록 없음
																		</p>
																	{/each}
																</div>
															{/each}
														</div>
													</section>
												{/each}
											</div>
										</section>
									{/if}
								</div>
							</details>
						{/if}
					</section>
				{:else}
					<div
						class="grid min-h-40 place-items-center text-center text-sm font-semibold text-slate-600"
					>
						아직 생성된 대진이 없습니다.
					</div>
				{/each}
			</div>
		</section>

		<section
			class="grid grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] xl:h-[min(40rem,calc(100dvh-16rem))] xl:min-h-[30rem]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<div class="flex items-center justify-between gap-3">
					<div class="min-w-0">
						<p class="text-base font-semibold text-slate-100">팀 구성</p>
						<p class="mt-1 text-xs text-slate-500">서버에 저장된 로스터와 포인트 내역</p>
					</div>
					<Medal size={18} class="text-amber-200" aria-hidden="true" />
				</div>
			</div>

			<div
				class="grid min-h-0 content-start gap-2 p-3 pr-[13px] xl:overflow-x-hidden xl:overflow-y-auto xl:overscroll-contain"
			>
				{#each orderedTeams as team (team.id)}
					<section class={`grid gap-3 border p-3 ${teamPanelClass(team)}`}>
						<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
							<div class="min-w-0">
								<div class="flex min-w-0 items-center gap-2">
									{#if team.id === snapshot.tournament.championTeamId}
										<Trophy size={18} class="shrink-0 text-amber-200" aria-hidden="true" />
									{/if}
									<h3 class="truncate text-xl font-semibold text-slate-50">{team.teamName}</h3>
								</div>
								<p class="mt-1 truncate text-xs font-semibold text-slate-500">
									{captainName(team)} 팀장 · {team.members.length}명
								</p>
							</div>
							<div class="grid grid-cols-3 gap-1.5 text-right">
								<div class="bg-slate-950/30 px-2 py-1.5">
									<p class="text-[10px] font-semibold text-slate-500">시작</p>
									<p class="font-mono text-sm font-bold tabular-nums text-slate-100">
										{pointLabel(team.initialPoints)}
									</p>
								</div>
								<div class="bg-slate-950/30 px-2 py-1.5">
									<p class="text-[10px] font-semibold text-slate-500">사용</p>
									<p class="font-mono text-sm font-bold tabular-nums text-amber-100">
										{pointLabel(team.spentPoints)}
									</p>
								</div>
								<div class="bg-slate-950/30 px-2 py-1.5">
									<p class="text-[10px] font-semibold text-slate-500">잔여</p>
									<p class="font-mono text-sm font-bold tabular-nums text-slate-100">
										{pointLabel(team.currentPoints)}
									</p>
								</div>
							</div>
						</div>

						<div class="grid gap-1.5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-2 2xl:grid-cols-3">
							{#each teamMembers(team) as member (member.teamMemberId)}
								<div
									class="grid min-h-20 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 bg-slate-950/25 p-2"
								>
									<span
										class="grid size-8 place-items-center border border-slate-600 bg-slate-950 text-xs font-bold text-slate-200"
									>
										{#if member.declaredLane}
											<PositionIcon role={member.declaredLane} size="sm" />
										{:else}
											{memberInitial(member.displayName)}
										{/if}
									</span>
									<span class="min-w-0">
										<span class="block truncate text-sm font-semibold text-slate-100">
											{member.displayName}
										</span>
										<span class="block truncate text-xs font-semibold text-slate-500">
											{declaredLaneLabel(member.declaredLane)} · {memberAcquisitionLabel(member)}
										</span>
									</span>
								</div>
							{:else}
								<div
									class="grid min-h-20 place-items-center bg-slate-950/25 text-sm font-semibold text-slate-600"
								>
									로스터 배정 대기
								</div>
							{/each}
						</div>
					</section>
				{/each}
			</div>
		</section>
	</div>

	<section
		class="grid border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
	>
		<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
			<div class="flex items-center justify-between gap-3">
				<div class="min-w-0">
					<p class="text-base font-semibold text-slate-100">참가자 정산</p>
					<p class="mt-1 text-xs text-slate-500">확정된 게임 기준의 서버 MMR 반영 내역</p>
				</div>
				<ReceiptText size={18} class="text-slate-400" aria-hidden="true" />
			</div>
		</div>

		<div class="overflow-x-auto">
			<div class="min-w-[760px]">
				<div
					class="grid grid-cols-[3rem_minmax(0,1.2fr)_minmax(0,0.8fr)_6rem_5.5rem_6rem] border-b border-[var(--color-app-border-muted)] px-3 py-2 text-[11px] font-bold uppercase text-slate-500"
				>
					<span>#</span>
					<span>참가자</span>
					<span>팀</span>
					<span>라인</span>
					<span class="text-right">전적</span>
					<span class="text-right">MMR 변화</span>
				</div>

				{#each playerRows as player, index (player.userId)}
					{@const team = teamById(player.teamId)}
					{@const primaryLane = playerPrimaryLane(player)}
					<div
						class="grid min-h-14 grid-cols-[3rem_minmax(0,1.2fr)_minmax(0,0.8fr)_6rem_5.5rem_6rem] items-center border-b border-[var(--color-app-border-muted)] px-3 py-2 text-sm last:border-b-0"
					>
						<span class="font-mono text-xs font-bold tabular-nums text-slate-500">{index + 1}</span>
						<span class="flex min-w-0 items-center gap-2">
							<span
								class="grid size-7 place-items-center border border-slate-600 bg-slate-950 text-xs font-bold text-slate-200"
							>
								{#if primaryLane}
									<PositionIcon role={primaryLane} size="sm" />
								{:else}
									{memberInitial(player.displayName)}
								{/if}
							</span>
							<span class="block min-w-0 truncate font-semibold text-slate-100"
								>{player.displayName}</span
							>
						</span>
						<span class="truncate font-semibold text-slate-200"
							>{team?.teamName ?? '팀 정보 없음'}</span
						>
						<span
							class="inline-flex min-w-0 items-center gap-1 text-xs font-semibold text-slate-400"
						>
							{#if primaryLane}
								<PositionIcon role={primaryLane} size="sm" />
							{/if}
							<span>{playerLaneLabel(player)}</span>
						</span>
						<span class="text-right font-mono font-bold tabular-nums text-slate-300">
							{player.wins}-{player.losses}
						</span>
						<span
							class={`text-right font-mono font-bold tabular-nums ${mmrDeltaClass(player.mmrDelta)}`}
						>
							{mmrDeltaLabel(player.mmrDelta)}
						</span>
					</div>
				{:else}
					<div
						class="grid min-h-28 place-items-center px-3 text-center text-sm font-semibold text-slate-600"
					>
						아직 정산할 확정 게임이 없습니다.
					</div>
				{/each}
			</div>
		</div>
	</section>

	<div
		class="border-y border-[var(--color-app-border-muted)] bg-slate-950/20 px-3 py-3 text-xs font-semibold text-slate-500"
	>
		MMR 변화가 미반영으로 표시되면 해당 선수의 확정 MMR 반영값을 아직 사용할 수 없습니다.
	</div>
</div>
