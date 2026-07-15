<script lang="ts">
	import { Check, Lock, MousePointer2, X } from 'lucide-svelte';
	import type {
		AuctionBracketSlotKey,
		AuctionTournamentBracketCommand,
		AuctionTournamentBracketMatch,
		AuctionTournamentBracketSnapshot,
		AuctionTournamentBracketTeam
	} from '$lib/contracts/auctionTournamentBracket';
	import PositionIcon from './PositionIcon.svelte';

	type BracketStage = 'SEMIFINAL_A' | 'SEMIFINAL_B' | 'FINAL';

	type BracketDisplayMatch = {
		matchKey: BracketStage;
		label: string;
		slotKeys: [AuctionBracketSlotKey, AuctionBracketSlotKey];
		bestOf: 1 | 3 | 5 | null;
	};

	interface Props {
		snapshot: AuctionTournamentBracketSnapshot;
		onCommand: (command: AuctionTournamentBracketCommand) => void | Promise<void>;
		onOpenMatch: (matchId: string) => void | Promise<void>;
		busy?: boolean;
	}

	let { snapshot, onCommand, onOpenMatch, busy = false }: Props = $props();

	const isDirectFinal = $derived(snapshot.tournament.format === 'TEN_PERSON');
	const canEdit = $derived(
		snapshot.tournament.status === 'BRACKET_SETUP' &&
			snapshot.permissions.canManageBracket &&
			!snapshot.bracket.locked &&
			!busy
	);
	const slotsByKey = $derived(
		new Map(snapshot.bracket.slots.map((slot) => [slot.slotKey, slot.teamId]))
	);
	const teamsById = $derived(new Map(snapshot.teams.map((team) => [team.id, team])));
	const activeSlotKeys = $derived<readonly AuctionBracketSlotKey[]>(
		isDirectFinal ? ['A_0', 'A_1'] : ['A_0', 'A_1', 'B_0', 'B_1']
	);
	const bracketMatches = $derived<BracketDisplayMatch[]>(
		isDirectFinal
			? [
					{
						matchKey: 'FINAL',
						label: '결승',
						slotKeys: ['A_0', 'A_1'],
						bestOf: snapshot.tournament.rules.finalBestOf
					}
				]
			: [
					{
						matchKey: 'SEMIFINAL_A',
						label: '준결승 A',
						slotKeys: ['A_0', 'A_1'],
						bestOf: snapshot.tournament.rules.semifinalBestOf
					},
					{
						matchKey: 'SEMIFINAL_B',
						label: '준결승 B',
						slotKeys: ['B_0', 'B_1'],
						bestOf: snapshot.tournament.rules.semifinalBestOf
					}
				]
	);
	const assignedTeamIds = $derived(
		activeSlotKeys.flatMap((slotKey) => {
			const teamId = slotsByKey.get(slotKey);
			return teamId ? [teamId] : [];
		})
	);
	const availableTeams = $derived(
		snapshot.teams.filter((team) => !assignedTeamIds.includes(team.id))
	);
	const ready = $derived(activeSlotKeys.every((slotKey) => Boolean(slotsByKey.get(slotKey))));
	const orderedMatches = $derived(
		snapshot.bracket.matches.slice().sort((left, right) => matchOrder(left) - matchOrder(right))
	);

	let selectedTeamId = $state<string | null>(null);
	let draggedTeamId = $state<string | null>(null);
	let overSlotKey = $state<AuctionBracketSlotKey | null>(null);

	function matchOrder(match: AuctionTournamentBracketMatch): number {
		if (match.matchKey === 'SEMIFINAL_A') return 0;
		if (match.matchKey === 'SEMIFINAL_B') return 1;
		return 2;
	}

	function teamById(teamId: string | null | undefined): AuctionTournamentBracketTeam | null {
		return teamId ? (teamsById.get(teamId) ?? null) : null;
	}

	function slotTeam(slotKey: AuctionBracketSlotKey): AuctionTournamentBracketTeam | null {
		return teamById(slotsByKey.get(slotKey));
	}

	function teamInitial(displayName: string): string {
		return displayName.trim().slice(0, 1).toLocaleUpperCase('ko-KR') || '?';
	}

	function captainName(team: AuctionTournamentBracketTeam): string {
		return (
			team.members.find((member) => member.acquiredVia === 'CAPTAIN')?.displayName ?? '팀장 미지정'
		);
	}

	function matchBestOf(display: BracketDisplayMatch): 1 | 3 | 5 | null {
		return (
			snapshot.bracket.matches.find((match) => match.matchKey === display.matchKey)?.bestOf ??
			display.bestOf
		);
	}

	function selectedTeam(): AuctionTournamentBracketTeam | null {
		return teamById(selectedTeamId);
	}

	function resetTransientState() {
		selectedTeamId = null;
		draggedTeamId = null;
		overSlotKey = null;
	}

	async function submit(command: AuctionTournamentBracketCommand): Promise<boolean> {
		if (!canEdit) return false;
		try {
			await onCommand(command);
			resetTransientState();
			return true;
		} catch {
			// The parent owns the safe backend error and retains the last server snapshot.
			return false;
		}
	}

	function assignTeam(targetSlotKey: AuctionBracketSlotKey, teamId: string) {
		if (!canEdit || !teamById(teamId)) return;
		if (slotsByKey.get(targetSlotKey) === teamId) {
			selectedTeamId = null;
			return;
		}
		void submit({
			kind: 'ASSIGN_TEAM',
			slotKey: targetSlotKey,
			teamId,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function clearSlot(targetSlotKey: AuctionBracketSlotKey) {
		if (!canEdit || !slotsByKey.get(targetSlotKey)) return;
		void submit({
			kind: 'CLEAR_SLOT',
			slotKey: targetSlotKey,
			expectedRevision: snapshot.tournament.revision
		});
	}

	function lockBracket() {
		if (!canEdit || !ready) return;
		void submit({ kind: 'LOCK_BRACKET', expectedRevision: snapshot.tournament.revision });
	}

	function selectTeam(teamId: string) {
		if (!canEdit || !teamById(teamId)) return;
		selectedTeamId = selectedTeamId === teamId ? null : teamId;
	}

	function activateSlot(slotKey: AuctionBracketSlotKey) {
		if (!canEdit) return;
		if (selectedTeamId) {
			assignTeam(slotKey, selectedTeamId);
			return;
		}
		const team = slotTeam(slotKey);
		if (team) selectTeam(team.id);
	}

	function startTeamDrag(event: globalThis.DragEvent, teamId: string) {
		if (!canEdit || !event.dataTransfer) return;
		draggedTeamId = teamId;
		event.dataTransfer.setData('text/plain', teamId);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		draggedTeamId = null;
		overSlotKey = null;
	}

	function handleSlotDragOver(event: globalThis.DragEvent, targetSlotKey: AuctionBracketSlotKey) {
		if (!canEdit || !event.dataTransfer || !draggedTeamId) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = 'move';
		overSlotKey = targetSlotKey;
	}

	function handleSlotDrop(event: globalThis.DragEvent, targetSlotKey: AuctionBracketSlotKey) {
		if (!canEdit || !event.dataTransfer) return;
		event.preventDefault();
		const teamId = event.dataTransfer.getData('text/plain');
		if (teamId) assignTeam(targetSlotKey, teamId);
		clearDragState();
	}

	function slotClass(targetSlotKey: AuctionBracketSlotKey, occupied: boolean): string {
		if (overSlotKey === targetSlotKey) {
			return 'border-cyan-300/60 bg-cyan-400/12 ring-1 ring-cyan-300/35';
		}
		if (selectedTeamId && !occupied) {
			return 'border-cyan-300/35 bg-cyan-400/5';
		}
		if (!occupied) return 'border-dashed border-slate-700 bg-slate-950/25';
		return 'border-[var(--color-app-border-muted)] bg-slate-950/35';
	}

	function canOpenMatch(match: AuctionTournamentBracketMatch): boolean {
		return match.leftTeamId !== null && match.rightTeamId !== null && match.status !== 'CANCELLED';
	}

	function openMatch(match: AuctionTournamentBracketMatch) {
		if (!canOpenMatch(match)) return;
		void onOpenMatch(match.id);
	}

	function matchLabel(match: AuctionTournamentBracketMatch): string {
		if (match.matchKey === 'SEMIFINAL_A') return '준결승 A';
		if (match.matchKey === 'SEMIFINAL_B') return '준결승 B';
		return '결승';
	}

	function matchStatusLabel(match: AuctionTournamentBracketMatch): string {
		if (match.status === 'LINEUP_PENDING') return '라인업 대기';
		if (match.status === 'READY') return '시작 준비';
		if (match.status === 'IN_PROGRESS') return '진행 중';
		if (match.status === 'COMPLETED') return '완료';
		return '취소됨';
	}

	function unavailableMatchLabel(match: AuctionTournamentBracketMatch): string {
		return match.status === 'CANCELLED' ? '경기 취소됨' : '상대 확정 대기';
	}
</script>

<div class="grid gap-2 pb-2" aria-busy={busy}>
	<div class="grid gap-2 2xl:grid-cols-[minmax(22rem,28rem)_minmax(0,1fr)]">
		<section
			class="grid grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] 2xl:h-[min(40rem,calc(100dvh-16rem))] 2xl:min-h-[30rem]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<p class="text-base font-semibold text-slate-100">팀 풀</p>
				<p class="mt-1 text-xs text-slate-500">
					{#if canEdit}
						팀을 선택한 뒤 빈 슬롯을 누르거나 슬롯으로 드래그하세요.
					{:else if snapshot.bracket.locked}
						대진이 확정되었습니다.
					{:else}
						브래킷 편집 권한이 없거나 대진 설정 단계가 아닙니다.
					{/if}
				</p>
			</div>

			<div
				class="grid min-h-0 content-start gap-2 p-3 pr-[13px] 2xl:overflow-x-hidden 2xl:overflow-y-auto 2xl:overscroll-contain"
			>
				{#each availableTeams as team (team.id)}
					<button
						type="button"
						draggable={canEdit}
						disabled={!canEdit}
						aria-pressed={selectedTeamId === team.id}
						onclick={() => selectTeam(team.id)}
						ondragstart={(event) => startTeamDrag(event, team.id)}
						ondragend={clearDragState}
						class={`grid min-h-20 w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border px-3 py-3 text-left transition enabled:cursor-grab enabled:active:cursor-grabbing disabled:cursor-default ${
							selectedTeamId === team.id
								? 'border-cyan-300/55 bg-cyan-400/10 ring-1 ring-cyan-300/30'
								: 'border-transparent bg-slate-950/20 hover:border-cyan-300/25 hover:bg-cyan-400/5'
						}`}
					>
						<span class="min-w-0">
							<span class="block truncate text-xl font-semibold text-slate-100"
								>{team.teamName}</span
							>
							<span class="mt-1 block truncate text-sm font-medium text-slate-500">
								{captainName(team)} 팀장 · {team.members.length}/5명
							</span>
						</span>
						<span
							class="flex shrink-0 items-center -space-x-2"
							aria-label={`${team.teamName} 구성원`}
						>
							{#each team.members as member (member.teamMemberId)}
								<span
									class="grid size-9 place-items-center rounded-full border border-slate-600 bg-slate-950 text-xs font-bold text-slate-200"
									title={member.displayName}
								>
									{#if member.declaredLane}
										<PositionIcon role={member.declaredLane} size="md" />
									{:else}
										{teamInitial(member.displayName)}
									{/if}
								</span>
							{/each}
						</span>
					</button>
				{:else}
					<div
						class="grid min-h-40 place-items-center text-center text-sm font-semibold text-slate-600"
					>
						모든 팀이 대진에 배치되었습니다.
					</div>
				{/each}
			</div>
		</section>

		<section
			class="grid grid-rows-[auto_minmax(0,1fr)_auto] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] 2xl:h-[min(40rem,calc(100dvh-16rem))] 2xl:min-h-[30rem]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<p class="text-base font-semibold text-slate-100">대진 설정</p>
				<p class="mt-1 text-xs text-slate-500">
					{isDirectFinal
						? '2팀이 결승 한 경기로 진행합니다.'
						: '두 준결승의 승자가 결승에서 만납니다.'}
				</p>
			</div>

			<div
				class="grid min-h-0 content-start gap-3 p-3 pr-[13px] 2xl:overflow-x-hidden 2xl:overflow-y-auto 2xl:overscroll-contain"
			>
				{#each bracketMatches as match (match.matchKey)}
					<section class="grid min-h-0 content-start gap-3 bg-slate-950/15 p-3">
						<div class="flex items-center justify-between gap-2">
							<p class="text-sm font-semibold text-cyan-100">{match.label}</p>
							<p class="shrink-0 text-xs font-semibold text-slate-500">
								{matchBestOf(match) === null ? '규칙 미설정' : `BO${matchBestOf(match)}`}
							</p>
						</div>

						<div class="grid items-stretch gap-2 lg:grid-cols-[minmax(0,1fr)_4rem_minmax(0,1fr)]">
							{#each match.slotKeys as slotKey, index (slotKey)}
								{@const team = slotTeam(slotKey)}
								<div
									class={`grid h-44 grid-rows-[auto_minmax(0,1fr)] gap-3 border px-3 py-3 transition ${slotClass(slotKey, Boolean(team))}`}
									ondragover={(event) => handleSlotDragOver(event, slotKey)}
									ondragleave={() => {
										if (overSlotKey === slotKey) overSlotKey = null;
									}}
									ondrop={(event) => handleSlotDrop(event, slotKey)}
									role="group"
									aria-label={`${match.label} ${index === 0 ? '왼쪽' : '오른쪽'} 팀 슬롯`}
								>
									<div class="flex items-center justify-between gap-2">
										<p class="text-xs font-semibold text-slate-500">
											{index === 0 ? 'LEFT' : 'RIGHT'}
										</p>
										{#if team}
											<button
												type="button"
												class="grid size-9 place-items-center border border-rose-300/25 bg-rose-500/10 text-rose-200 hover:border-rose-300/45 hover:bg-rose-500/16 disabled:cursor-not-allowed disabled:opacity-40"
												aria-label={`${team.teamName} 슬롯 비우기`}
												disabled={!canEdit}
												onclick={() => clearSlot(slotKey)}
											>
												<X size={17} aria-hidden="true" />
											</button>
										{/if}
									</div>

									<button
										type="button"
										class={`grid min-h-0 w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 overflow-hidden text-left disabled:cursor-default ${
											selectedTeamId === team?.id ? 'text-cyan-100' : 'text-slate-100'
										}`}
										disabled={!canEdit}
										onclick={() => activateSlot(slotKey)}
									>
										{#if team}
											<span class="min-w-0">
												<span class="block truncate text-3xl font-semibold">{team.teamName}</span>
												<span class="mt-1 block truncate text-sm font-medium text-slate-500">
													{captainName(team)} 팀장 · {team.members.length}/5명
												</span>
											</span>
											<span class="flex shrink-0 -space-x-2">
												{#each team.members as member (member.teamMemberId)}
													<span
														class="grid size-9 place-items-center rounded-full border border-slate-600 bg-slate-950 text-xs font-bold text-slate-200"
														title={member.displayName}
													>
														{#if member.declaredLane}
															<PositionIcon role={member.declaredLane} size="md" />
														{:else}
															{teamInitial(member.displayName)}
														{/if}
													</span>
												{/each}
											</span>
										{:else}
											<span
												class="grid min-h-20 place-items-center bg-slate-950/25 text-base font-semibold text-slate-600"
											>
												{selectedTeam() ? `${selectedTeam()?.teamName} 배치` : '팀 배치'}
											</span>
										{/if}
									</button>
								</div>

								{#if index === 0}
									<div
										class="grid min-h-16 place-items-center text-xl font-black text-slate-500 lg:min-h-44"
									>
										VS
									</div>
								{/if}
							{/each}
						</div>
					</section>
				{/each}

				{#if snapshot.bracket.locked && orderedMatches.length > 0}
					<section
						class="grid gap-2 border border-[var(--color-app-border-muted)] bg-slate-950/15 p-3"
					>
						<div class="flex items-center justify-between gap-2">
							<p class="text-sm font-semibold text-slate-100">진행할 경기</p>
							<span class="text-xs font-semibold text-emerald-200">대진 확정</span>
						</div>
						<div class="grid gap-2 sm:grid-cols-2">
							{#each orderedMatches as match (match.id)}
								{@const leftTeam = teamById(match.leftTeamId)}
								{@const rightTeam = teamById(match.rightTeamId)}
								{#if canOpenMatch(match)}
									<button
										type="button"
										class="grid min-h-24 gap-1 border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] px-3 py-2 text-left transition hover:border-cyan-300/35 hover:bg-cyan-400/5"
										onclick={() => openMatch(match)}
										aria-label={`${matchLabel(match)} 라인업 작업 열기`}
									>
										<span
											class="flex items-center justify-between gap-2 text-xs font-semibold text-slate-500"
										>
											{matchLabel(match)} · BO{match.bestOf}
											<span>{matchStatusLabel(match)}</span>
										</span>
										<span class="truncate text-base font-semibold text-slate-100">
											{leftTeam?.teamName ?? '대기'} <span class="px-1 text-slate-600">vs</span>
											{rightTeam?.teamName ?? '대기'}
										</span>
										<span class="text-xs font-medium text-cyan-100">라인업 작업 열기</span>
										{#if match.hardFearless}
											<span class="text-xs font-medium text-amber-100">하드 피어리스</span>
										{/if}
									</button>
								{:else}
									<div
										class="grid min-h-24 gap-1 border border-[var(--color-app-border-muted)] bg-slate-950/25 px-3 py-2"
									>
										<span
											class="flex items-center justify-between gap-2 text-xs font-semibold text-slate-500"
										>
											{matchLabel(match)} · BO{match.bestOf}
											<span>{matchStatusLabel(match)}</span>
										</span>
										<span class="truncate text-base font-semibold text-slate-500">
											{leftTeam?.teamName ?? '대기'} <span class="px-1 text-slate-700">vs</span>
											{rightTeam?.teamName ?? '대기'}
										</span>
										<span class="text-xs font-medium text-slate-600"
											>{unavailableMatchLabel(match)}</span
										>
									</div>
								{/if}
							{/each}
						</div>
					</section>
				{/if}
			</div>

			<div class="border-t border-[var(--color-app-border-muted)] p-3">
				{#if snapshot.bracket.locked}
					<div>
						<div
							class="flex min-h-12 items-center justify-center gap-2 border border-emerald-300/25 bg-emerald-400/8 px-4 text-sm font-semibold text-emerald-100"
						>
							<Check size={16} aria-hidden="true" /> 대진 확정됨
						</div>
					</div>
				{:else}
					<button
						type="button"
						class="inline-flex h-12 w-full items-center justify-center gap-2 border border-emerald-300/35 bg-emerald-400/14 px-4 text-base font-semibold text-emerald-100 hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
						disabled={!canEdit || !ready}
						onclick={lockBracket}
					>
						{#if selectedTeam()}
							<MousePointer2 size={16} aria-hidden="true" /> {selectedTeam()?.teamName} 배치 대기
						{:else}
							<Lock size={16} aria-hidden="true" /> {ready ? '대진 확정' : '팀 배치 필요'}
						{/if}
					</button>
				{/if}
			</div>
		</section>
	</div>
</div>
