<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { Check, ChevronDown, LoaderCircle, ShieldAlert, Undo2 } from 'lucide-svelte';
	import { championIconUrl, type ChampionCatalog } from '$lib/datadragon';
	import {
		auctionMatchResultTeams,
		validateAuctionMatchGameDraft,
		type AuctionMatchGameBans,
		type AuctionMatchGameResultRevertCommand,
		type AuctionMatchGameResultSnapshot,
		type AuctionMatchGameResultSubmitCommand,
		type AuctionMatchResultTeam
	} from '$lib/contracts/auctionMatchGameResult';
	import type { ChampionPreferenceLane } from '$lib/contracts/workflow';
	import PositionIcon from './PositionIcon.svelte';

	type Side = 'BLUE' | 'RED';
	type SidePreset = 'DEFAULT' | 'SWAPPED';
	type InputStep = 'SIDE' | 'PICKBAN' | 'WINNER';

	interface Props {
		snapshot: AuctionMatchGameResultSnapshot;
		catalog: ChampionCatalog;
		busy?: boolean;
		onSubmit?: (command: AuctionMatchGameResultSubmitCommand) => void | Promise<void>;
		onDeleteLastGame?: (command: AuctionMatchGameResultRevertCommand) => void | Promise<void>;
	}

	let { snapshot, catalog, busy = false, onSubmit, onDeleteLastGame }: Props = $props();

	const lanes: ChampionPreferenceLane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const sides: Side[] = ['BLUE', 'RED'];
	const sidePresets: SidePreset[] = ['DEFAULT', 'SWAPPED'];

	let inputStep = $state<InputStep>('SIDE');
	let sidePreset = $state<SidePreset>('DEFAULT');
	let winner = $state<Side | null>(null);
	let picks = $state<Record<string, string | null>>({});
	let bans = $state<AuctionMatchGameBans>(emptyBans());
	let patchVersion = $state<string | null>(null);
	let currentSetExpanded = $state(true);
	let submitting = $state(false);
	let reverting = $state(false);
	let revertDialogOpen = $state(false);
	let operationError = $state('');
	let syncedSnapshotKey = $state<string | null>(null);

	const resultTeams = $derived(
		auctionMatchResultTeams(snapshot).map((team) => ({
			...team,
			members: [...team.members].sort(
				(left, right) => lanes.indexOf(left.lane) - lanes.indexOf(right.lane)
			)
		}))
	);
	const nextSetNumber = $derived(Math.max(0, ...snapshot.games.map((game) => game.setNumber)) + 1);
	const requiredWins = $derived(Math.floor(snapshot.match.bestOf / 2) + 1);
	const matchDecided = $derived(
		snapshot.match.leftScore >= requiredWins || snapshot.match.rightScore >= requiredWins
	);
	const editingLocked = $derived(busy || submitting || reverting);
	const canEnterResults = $derived(
		Boolean(onSubmit) &&
			snapshot.permissions.canEnterResults &&
			snapshot.match.lineupsLocked &&
			snapshot.match.status !== 'COMPLETED' &&
			snapshot.match.status !== 'CANCELLED' &&
			nextSetNumber <= snapshot.match.bestOf &&
			!matchDecided &&
			!editingLocked
	);
	const canDeleteLastGame = $derived(
		Boolean(onDeleteLastGame) &&
			snapshot.games.length > 0 &&
			(snapshot.permissions.canRevertLastGame ?? snapshot.permissions.canEnterResults) &&
			!editingLocked
	);
	const draftConflicts = $derived(validateAuctionMatchGameDraft(snapshot, { picks, bans }));
	const hasDraftConflicts = $derived(
		draftConflicts.duplicateChampionIds.length > 0 ||
			draftConflicts.hardFearlessChampionIds.length > 0
	);
	const canSubmit = $derived(canEnterResults && winner !== null && !hasDraftConflicts);
	const rulesLabel = $derived(
		`BO${snapshot.match.bestOf} · ${snapshot.match.hardFearless ? '하드 피어리스' : '일반'}`
	);
	const snapshotKey = $derived(
		JSON.stringify({
			matchId: snapshot.match.id,
			revision: snapshot.match.revision,
			lineups: snapshot.lineups,
			games: snapshot.games,
			catalogVersion: catalog.version
		})
	);

	$effect(() => {
		if (snapshotKey === syncedSnapshotKey) return;
		syncedSnapshotKey = snapshotKey;
		inputStep = 'SIDE';
		sidePreset = 'DEFAULT';
		winner = null;
		picks = Object.fromEntries(snapshot.lineups.map((lineup) => [lineup.userId, null]));
		bans = emptyBans();
		patchVersion = catalog.version;
		operationError = '';
	});

	function emptyBans(): AuctionMatchGameBans {
		return { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] };
	}

	function sideForTeam(team: AuctionMatchResultTeam): Side {
		const isLeftTeam = team.id === snapshot.match.leftTeamId;
		if (sidePreset === 'DEFAULT') return isLeftTeam ? 'BLUE' : 'RED';
		return isLeftTeam ? 'RED' : 'BLUE';
	}

	function teamForSide(side: Side): AuctionMatchResultTeam | null {
		return resultTeams.find((team) => sideForTeam(team) === side) ?? null;
	}

	function sideClass(side: Side): string {
		return side === 'BLUE' ? 'text-blue-100' : 'text-red-100';
	}

	function sidePanelClass(side: Side): string {
		return side === 'BLUE'
			? 'border-blue-300/20 bg-blue-400/[0.045]'
			: 'border-red-300/20 bg-red-400/[0.045]';
	}

	function championName(championId: string): string {
		return catalog.byId[championId]?.name ?? championId;
	}

	function championIdsInDraft(): Array<{ cellKey: string; championId: string }> {
		return [
			...Object.entries(picks).flatMap(([userId, championId]) =>
				championId ? [{ cellKey: `pick:${userId}`, championId }] : []
			),
			...sides.flatMap((side) =>
				bans[side].flatMap((championId, index) =>
					championId ? [{ cellKey: `ban:${side}:${index}`, championId }] : []
				)
			)
		];
	}

	function isChampionUnavailable(championId: string, currentCellKey: string): boolean {
		const inAnotherCurrentCell = championIdsInDraft().some(
			(entry) => entry.cellKey !== currentCellKey && entry.championId === championId
		);
		if (inAnotherCurrentCell) return true;
		if (!snapshot.match.hardFearless || !currentCellKey.startsWith('pick:')) return false;
		return snapshot.games.some((game) => Object.values(game.picks).includes(championId));
	}

	function setSidePreset(nextPreset: SidePreset) {
		if (!canEnterResults) return;
		sidePreset = nextPreset;
		operationError = '';
	}

	function setBan(side: Side, index: number, value: string) {
		if (!canEnterResults) return;
		bans = {
			...bans,
			[side]: bans[side].map((championId, currentIndex) =>
				currentIndex === index ? value || null : championId
			)
		};
		operationError = '';
	}

	function setPick(userId: string, value: string) {
		if (!canEnterResults) return;
		picks = { ...picks, [userId]: value || null };
		operationError = '';
	}

	function setWinner(side: Side) {
		if (!canEnterResults) return;
		winner = side;
		operationError = '';
	}

	function setInputStep(nextStep: InputStep) {
		if (nextStep === 'WINNER' && hasDraftConflicts) return;
		inputStep = nextStep;
	}

	function callbackErrorText(cause: unknown, fallback: string): string {
		if (!(cause instanceof Error)) return fallback;
		const details = cause as unknown as { requestId?: unknown };
		const requestId = typeof details.requestId === 'string' ? details.requestId : null;
		return requestId && !cause.message.includes(requestId)
			? `${cause.message} (요청 ID: ${requestId})`
			: cause.message;
	}

	async function submitResult(): Promise<void> {
		if (!canSubmit || !onSubmit || winner === null) return;
		submitting = true;
		operationError = '';
		try {
			await onSubmit({
				expectedRevision: snapshot.match.revision,
				setNumber: nextSetNumber,
				sidePreset,
				winner,
				picks: { ...picks },
				bans: { BLUE: [...bans.BLUE], RED: [...bans.RED] },
				patchVersion
			});
		} catch (cause) {
			operationError = callbackErrorText(cause, '경매 게임 결과를 기록하지 못했습니다.');
		} finally {
			submitting = false;
		}
	}

	async function deleteLastGame(): Promise<void> {
		if (!canDeleteLastGame || !onDeleteLastGame) return;
		reverting = true;
		operationError = '';
		try {
			await onDeleteLastGame({ expectedRevision: snapshot.match.revision });
			revertDialogOpen = false;
		} catch (cause) {
			operationError = callbackErrorText(cause, '마지막 경매 게임 결과를 되돌리지 못했습니다.');
		} finally {
			reverting = false;
		}
	}

	function winnerLabel(game: AuctionMatchGameResultSnapshot['games'][number]): string {
		const leftWon =
			(game.sidePreset === 'DEFAULT' && game.winner === 'BLUE') ||
			(game.sidePreset === 'SWAPPED' && game.winner === 'RED');
		const team = leftWon ? resultTeams[0] : resultTeams[1];
		return `${team?.teamName ?? game.winner} 승리`;
	}

	function statusLabel(): string {
		if (snapshot.match.status === 'LINEUP_PENDING') return '라인업 대기';
		if (snapshot.match.status === 'READY') return '경기 준비';
		if (snapshot.match.status === 'IN_PROGRESS') return '경기 진행 중';
		if (snapshot.match.status === 'COMPLETED') return '완료';
		return '취소됨';
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-12 flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--color-app-border-muted)] px-3 py-2"
	>
		<button
			type="button"
			class="flex min-w-0 flex-1 items-center justify-between gap-3 text-left hover:text-slate-100"
			aria-expanded={currentSetExpanded}
			onclick={() => (currentSetExpanded = !currentSetExpanded)}
		>
			<span class="min-w-0">
				<span class="block truncate text-sm font-semibold text-slate-100">
					{snapshot.match.label} 결과 입력
				</span>
				<span class="mt-0.5 block truncate text-[11px] font-medium text-slate-500">
					{rulesLabel} · {statusLabel()} · {snapshot.match.leftScore}:{snapshot.match.rightScore}
				</span>
			</span>
			<span
				class="inline-flex shrink-0 items-center gap-1 text-[11px] font-semibold text-slate-400"
			>
				{currentSetExpanded ? '접기' : `${nextSetNumber}세트 입력`}
				<ChevronDown
					size={15}
					class={`transition-transform ${currentSetExpanded ? 'rotate-180' : ''}`}
					aria-hidden="true"
				/>
			</span>
		</button>

		{#if snapshot.games.length > 0}
			<button
				type="button"
				disabled={!canDeleteLastGame}
				onclick={() => (revertDialogOpen = true)}
				class="inline-flex h-8 shrink-0 items-center justify-center gap-1.5 border border-amber-300/25 px-2.5 text-[11px] font-semibold text-amber-100 enabled:hover:bg-amber-400/10 disabled:cursor-not-allowed disabled:opacity-35"
			>
				{#if reverting}
					<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
				{:else}
					<Undo2 size={14} aria-hidden="true" /> 마지막 결과 되돌리기
				{/if}
			</button>
		{/if}
	</header>

	{#if currentSetExpanded}
		<div class="grid gap-2 p-3">
			{#if !snapshot.match.lineupsLocked}
				<p
					class="border border-amber-300/20 bg-amber-400/[0.06] px-3 py-2 text-xs leading-5 font-medium text-amber-100"
				>
					양 팀의 라인업을 확정한 뒤에만 게임 결과를 기록할 수 있습니다.
				</p>
			{:else if resultTeams.length !== 2}
				<p
					class="border border-rose-300/20 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 font-medium text-rose-100"
				>
					경기 라인업을 확인할 수 없습니다. 최신 경기 정보를 다시 불러와 주세요.
				</p>
			{:else if nextSetNumber > snapshot.match.bestOf || matchDecided}
				<p
					class="border border-emerald-300/20 bg-emerald-400/[0.06] px-3 py-2 text-xs leading-5 font-medium text-emerald-100"
				>
					이 경기의 승패가 이미 확정되었습니다.
				</p>
			{:else}
				<div class="grid grid-cols-3 gap-1" aria-label="게임 결과 입력 단계">
					{#each [{ key: 'SIDE' as const, label: '사이드' }, { key: 'PICKBAN' as const, label: '픽밴' }, { key: 'WINNER' as const, label: '승자' }] as step (step.key)}
						<button
							type="button"
							disabled={editingLocked || (step.key === 'WINNER' && hasDraftConflicts)}
							onclick={() => setInputStep(step.key)}
							class={`h-8 border text-xs font-semibold disabled:cursor-not-allowed disabled:opacity-35 ${
								inputStep === step.key
									? 'border-cyan-300/35 bg-cyan-400/12 text-cyan-100'
									: 'border-[var(--color-app-border-muted)] bg-slate-950/20 text-slate-400 enabled:hover:bg-slate-900/55'
							}`}
						>
							{step.label}
						</button>
					{/each}
				</div>

				{#if inputStep === 'SIDE'}
					<div class="grid gap-2">
						<div class="grid grid-cols-2 gap-1">
							{#each sidePresets as preset (preset)}
								<button
									type="button"
									disabled={!canEnterResults}
									onclick={() => setSidePreset(preset)}
									class={`h-9 border px-2 text-xs font-semibold disabled:cursor-not-allowed disabled:opacity-35 ${
										sidePreset === preset
											? 'border-cyan-300/35 bg-cyan-400/12 text-cyan-100'
											: 'border-[var(--color-app-border-muted)] bg-slate-950/20 text-slate-300 enabled:hover:bg-slate-900/55'
									}`}
								>
									{preset === 'DEFAULT' ? '왼쪽 팀 블루' : '오른쪽 팀 블루'}
								</button>
							{/each}
						</div>
						<div class="grid gap-2 xl:grid-cols-2">
							{#each resultTeams as team (team.id)}
								{@const side = sideForTeam(team)}
								<section class={`border p-2 ${sidePanelClass(side)}`}>
									<header
										class="flex items-center justify-between gap-2 border-b border-[var(--color-app-border-muted)] pb-2"
									>
										<p class="truncate text-sm font-semibold text-slate-100">{team.teamName}</p>
										<span class={`text-xs font-semibold ${sideClass(side)}`}>{side}</span>
									</header>
									<div class="mt-2 grid grid-cols-5 gap-1">
										{#each team.members as player (player.userId)}
											<div class="grid min-w-0 place-items-center gap-1 text-center">
												<PositionIcon role={player.lane} size="sm" />
												<p class="w-full truncate text-[11px] font-medium text-slate-300">
													{player.displayName}
												</p>
											</div>
										{/each}
									</div>
								</section>
							{/each}
						</div>
						<div class="flex justify-end">
							<button
								type="button"
								disabled={!canEnterResults}
								onclick={() => setInputStep('PICKBAN')}
								class="h-9 border border-cyan-300/35 bg-cyan-400/12 px-4 text-xs font-semibold text-cyan-100 enabled:hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-35"
							>
								다음
							</button>
						</div>
					</div>
				{:else if inputStep === 'PICKBAN'}
					<div class="grid gap-2">
						<div class="overflow-x-auto border border-[var(--color-app-border-muted)]">
							<div
								class="grid min-w-[46rem] gap-px bg-[var(--color-app-border-muted)] xl:min-w-0 xl:grid-cols-2"
							>
								{#each sides as side (side)}
									{@const team = teamForSide(side)}
									{#if team}
										<section class="min-w-0 bg-[var(--color-app-surface)] p-2">
											<header
												class="flex items-center justify-between gap-2 border-b border-[var(--color-app-border-muted)] pb-2"
											>
												<p class="truncate text-sm font-semibold text-slate-100">{team.teamName}</p>
												<span class={`text-xs font-semibold ${sideClass(side)}`}>{side}</span>
											</header>
											<div class="mt-2 grid gap-2">
												<div class="grid grid-cols-[2.25rem_repeat(5,minmax(0,1fr))] gap-1">
													<span
														class="grid place-items-center text-[11px] font-semibold text-slate-500"
														>밴</span
													>
													{#each bans[side] as ban, index (`${side}-${index}`)}
														{@const selected = ban ? catalog.byId[ban] : null}
														<div class="grid min-w-0 gap-1">
															{#if selected}
																<img
																	src={championIconUrl(selected, patchVersion ?? catalog.version)}
																	alt={selected.name}
																	class="mx-auto size-7 object-cover"
																	loading="lazy"
																/>
															{:else}
																<span class="mx-auto size-7 bg-slate-950/45"></span>
															{/if}
															<select
																class="h-8 min-w-0 border border-[var(--color-app-border-muted)] bg-slate-950/35 px-1 text-[11px] font-semibold text-slate-200 outline-none focus:border-cyan-300/45 disabled:cursor-not-allowed disabled:opacity-40"
																value={ban ?? ''}
																disabled={!canEnterResults}
																onchange={(event) => setBan(side, index, event.currentTarget.value)}
																aria-label={`${team.teamName} ${side} 밴 ${index + 1}`}
															>
																<option value="">선택</option>
																{#each catalog.champions as champion (champion.id)}
																	<option
																		value={champion.id}
																		disabled={isChampionUnavailable(
																			champion.id,
																			`ban:${side}:${index}`
																		)}
																	>
																		{champion.name}
																	</option>
																{/each}
															</select>
														</div>
													{/each}
												</div>

												<div class="grid gap-1.5">
													{#each team.members as player (player.userId)}
														{@const picked = picks[player.userId] ?? null}
														{@const selected = picked ? catalog.byId[picked] : null}
														<div
															class="grid min-h-14 grid-cols-[2rem_minmax(7rem,0.7fr)_2.25rem_minmax(9rem,1fr)] items-center gap-2 border border-[var(--color-app-border-muted)] bg-slate-950/20 px-2 py-1.5"
														>
															<PositionIcon role={player.lane} size="sm" />
															<p class="truncate text-sm font-semibold text-slate-100">
																{player.displayName}
															</p>
															{#if selected}
																<img
																	src={championIconUrl(selected, patchVersion ?? catalog.version)}
																	alt={selected.name}
																	class="size-8 object-cover"
																	loading="lazy"
																/>
															{:else}
																<span class="size-8 bg-slate-950/45"></span>
															{/if}
															<select
																class="h-9 min-w-0 border border-[var(--color-app-border-muted)] bg-slate-950/35 px-2 text-xs font-semibold text-slate-200 outline-none focus:border-cyan-300/45 disabled:cursor-not-allowed disabled:opacity-40"
																value={picked ?? ''}
																disabled={!canEnterResults}
																onchange={(event) =>
																	setPick(player.userId, event.currentTarget.value)}
																aria-label={`${team.teamName} ${player.displayName} 픽`}
															>
																<option value="">선택 안 함</option>
																{#each catalog.champions as champion (champion.id)}
																	<option
																		value={champion.id}
																		disabled={isChampionUnavailable(
																			champion.id,
																			`pick:${player.userId}`
																		)}
																	>
																		{champion.name}
																	</option>
																{/each}
															</select>
														</div>
													{/each}
												</div>
											</div>
										</section>
									{/if}
								{/each}
							</div>
						</div>

						{#if hasDraftConflicts}
							<div
								class="flex items-start gap-2 border border-rose-300/25 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 text-rose-100"
								role="alert"
							>
								<ShieldAlert size={16} class="mt-0.5 shrink-0" aria-hidden="true" />
								<div>
									{#if draftConflicts.duplicateChampionIds.length > 0}
										<p>
											같은 게임에서 중복 선택된 챔피언: {draftConflicts.duplicateChampionIds
												.map(championName)
												.join(', ')}
										</p>
									{/if}
									{#if draftConflicts.hardFearlessChampionIds.length > 0}
										<p>
											하드 피어리스에서 이전 확정 게임의 픽과 중복된 챔피언: {draftConflicts.hardFearlessChampionIds
												.map(championName)
												.join(', ')}
										</p>
									{/if}
								</div>
							</div>
						{/if}

						<div class="flex justify-between gap-2">
							<button
								type="button"
								disabled={editingLocked}
								onclick={() => setInputStep('SIDE')}
								class="h-9 border border-[var(--color-app-border-muted)] px-3 text-xs font-semibold text-slate-300 enabled:hover:bg-slate-900/55 disabled:cursor-not-allowed disabled:opacity-35"
							>
								이전
							</button>
							<button
								type="button"
								disabled={!canEnterResults || hasDraftConflicts}
								onclick={() => setInputStep('WINNER')}
								class="h-9 border border-cyan-300/35 bg-cyan-400/12 px-4 text-xs font-semibold text-cyan-100 enabled:hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-35"
							>
								다음
							</button>
						</div>
					</div>
				{:else}
					<div class="grid gap-2">
						<div class="grid gap-1.5 xl:grid-cols-2">
							{#each sides as side (side)}
								{@const team = teamForSide(side)}
								{#if team}
									<button
										type="button"
										disabled={!canEnterResults}
										onclick={() => setWinner(side)}
										class={`min-h-14 border px-3 text-left text-sm font-semibold disabled:cursor-not-allowed disabled:opacity-35 ${
											winner === side
												? 'border-emerald-300/35 bg-emerald-400/[0.10] text-emerald-100'
												: 'border-[var(--color-app-border-muted)] bg-slate-950/20 text-slate-300 enabled:hover:bg-slate-900/55'
										}`}
									>
										<span class={`block text-xs ${sideClass(side)}`}>{side}</span>
										<span class="mt-1 block truncate">{team.teamName} 승리</span>
									</button>
								{/if}
							{/each}
						</div>
						<div class="flex justify-between gap-2">
							<button
								type="button"
								disabled={editingLocked}
								onclick={() => setInputStep('PICKBAN')}
								class="h-9 border border-[var(--color-app-border-muted)] px-3 text-xs font-semibold text-slate-300 enabled:hover:bg-slate-900/55 disabled:cursor-not-allowed disabled:opacity-35"
							>
								이전
							</button>
							<button
								type="button"
								disabled={!canSubmit}
								onclick={() => void submitResult()}
								class="inline-flex h-9 items-center justify-center gap-1.5 border border-emerald-300/35 bg-emerald-400/[0.12] px-4 text-xs font-semibold text-emerald-100 enabled:hover:bg-emerald-400/[0.18] disabled:cursor-not-allowed disabled:opacity-35"
							>
								{#if submitting}
									<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 기록 중
								{:else}
									<Check size={14} aria-hidden="true" /> {nextSetNumber}세트 기록
								{/if}
							</button>
						</div>
					</div>
				{/if}
			{/if}

			{#if operationError}
				<p
					class="border border-rose-300/25 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 text-rose-100"
					role="alert"
				>
					{operationError}
				</p>
			{/if}

			{#if snapshot.games.length > 0}
				<details class="border border-[var(--color-app-border-muted)] bg-slate-950/15">
					<summary class="cursor-pointer px-3 py-2 text-xs font-semibold text-slate-300">
						입력된 세트 결과 {snapshot.games.length}개
					</summary>
					<div
						class="grid gap-px border-t border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)]"
					>
						{#each snapshot.games as game (game.id)}
							<div
								class="flex items-center justify-between gap-3 bg-[var(--color-app-surface)] px-3 py-2"
							>
								<span class="text-xs font-semibold text-slate-300">{game.setNumber}세트</span>
								<span class="truncate text-xs font-medium text-slate-500">
									{game.sidePreset === 'DEFAULT' ? '왼쪽 팀 블루' : '오른쪽 팀 블루'} · {winnerLabel(
										game
									)}
								</span>
							</div>
						{/each}
					</div>
				</details>
			{/if}
		</div>
	{/if}
</section>

<Dialog.Root bind:open={revertDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<Dialog.Title class="text-sm font-semibold text-slate-50"
				>마지막 게임 결과 되돌리기</Dialog.Title
			>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				가장 최근에 확정한 세트와 경기 점수, 이미 반영된 길드 MMR을 함께 되돌립니다. 완료된 결승의
				Discord 결과 메시지는 서버가 전송 상태를 확인해 정리하며, 확인할 수 없으면 이 작업은
				진행되지 않습니다.
			</Dialog.Description>
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={editingLocked}
					class="inline-flex h-8 items-center px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={!canDeleteLastGame}
					onclick={() => void deleteLastGame()}
					class="inline-flex h-8 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if reverting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<Undo2 size={14} aria-hidden="true" /> 결과 되돌리기
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
