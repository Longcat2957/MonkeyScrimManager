<script lang="ts">
	import { ChevronDown, ClipboardPenLine, Redo2, Undo2 } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import {
		mockScrimPickBanResultInputGames,
		mockScrimPickBanResultInputPatch,
		mockScrimPickBanResultInputTeams,
		scrimPickBanResultInputChampions,
		type ScrimPickBanResultInputChampion,
		type ScrimPickBanResultInputGame,
		type ScrimPickBanResultInputSidePreset,
		type ScrimPickBanResultInputTeam,
		type ScrimPickBanResultInputSide
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import type {
		SeriesPickBanBulkItem,
		SeriesPickBanDraftOperation
	} from '$lib/contracts/seriesPickBan';
	import PositionIcon from './PositionIcon.svelte';
	import ScrimPickBanBulkInput from './ScrimPickBanBulkInput.svelte';
	import {
		adjacentScrimPickBanResultInputCell,
		scrimPickBanResultInputCellId,
		scrimPickBanResultInputCellOrder,
		type ScrimPickBanResultInputActiveCell
	} from './scrimPickBanResultInputHelpers';
	import ScrimPickBanResultInputTeamPanel from './ScrimPickBanResultInputTeamPanel.svelte';

	interface Props {
		strict?: boolean;
		teams?: Record<ScrimPickBanResultInputSide, ScrimPickBanResultInputTeam>;
		initialGame?: ScrimPickBanResultInputGame;
		champions?: ScrimPickBanResultInputChampion[];
		patchVersion?: string;
		teamsAreGameSides?: boolean;
		hardFearless?: boolean;
		previousPickChampionIds?: readonly string[];
		validateDraft?: boolean;
		allowLocalBulkInput?: boolean;
		readOnly?: boolean;
		busy?: boolean;
		onDraftOperation?: (operation: SeriesPickBanDraftOperation) => void | Promise<void>;
		onSubmit?: (game: ScrimPickBanResultInputGame) => void | Promise<void>;
	}

	let {
		strict = false,
		teams = mockScrimPickBanResultInputTeams,
		initialGame = mockScrimPickBanResultInputGames[0]!,
		champions = scrimPickBanResultInputChampions,
		patchVersion = mockScrimPickBanResultInputPatch,
		teamsAreGameSides = false,
		hardFearless = false,
		previousPickChampionIds = [],
		validateDraft = false,
		allowLocalBulkInput = false,
		readOnly = false,
		busy = false,
		onDraftOperation,
		onSubmit
	}: Props = $props();

	type ScrimPickBanResultInputStep = 'SIDE' | 'PICKBAN' | 'WINNER';
	type ScrimPickBanResultInputSnapshot = {
		sidePreset: ScrimPickBanResultInputSidePreset;
		winner: ScrimPickBanResultInputSide | null;
		bans: Record<ScrimPickBanResultInputSide, Array<string | null>>;
		picks: Record<string, string | null>;
	};
	type ScrimPickBanResultInputDraftConflicts = {
		duplicateChampionIds: string[];
		hardFearlessChampionIds: string[];
	};

	let inputStep = $state<ScrimPickBanResultInputStep>('SIDE');
	let sidePreset = $state<ScrimPickBanResultInputSidePreset>(
		untrack(() => initialGame.sidePreset ?? 'DEFAULT')
	);
	let sideSelected = $state(untrack(() => hasExplicitSideSelection(initialGame)));
	let winner = $state<ScrimPickBanResultInputSide | null>(untrack(() => initialGame.winner));
	let bans = $state<Record<ScrimPickBanResultInputSide, Array<string | null>>>({
		BLUE: untrack(() => [...initialGame.bans.BLUE]),
		RED: untrack(() => [...initialGame.bans.RED])
	});
	let picks = $state<Record<string, string | null>>(untrack(() => ({ ...initialGame.picks })));
	let activeCell = $state<ScrimPickBanResultInputActiveCell>({
		kind: 'PICK',
		key: untrack(() => teams.BLUE.players[2]?.id ?? teams.BLUE.players[0]?.id ?? '')
	});
	let undoStack = $state<ScrimPickBanResultInputSnapshot[]>([]);
	let redoStack = $state<ScrimPickBanResultInputSnapshot[]>([]);
	let saved = $state(false);
	let submitting = $state(false);
	let submissionError = $state('');
	let bulkInputOpen = $state(false);
	let currentSetExpanded = $state(true);
	let lastSyncedGameFingerprint: string | null = null;
	let lastSyncedGameId: string | null = null;
	const currentSetDisclosureId = $props.id();

	const allBansFilled = $derived([...bans.BLUE, ...bans.RED].every(Boolean));
	const allPicksFilled = $derived(Object.values(picks).every(Boolean));
	const sideDone = $derived(sideSelected);
	const pickBanDone = $derived(allBansFilled && allPicksFilled);
	const draftConflicts = $derived(
		findDraftConflicts(picks, bans, hardFearless, previousPickChampionIds)
	);
	const hasDraftConflicts = $derived(
		draftConflicts.duplicateChampionIds.length > 0 ||
			draftConflicts.hardFearlessChampionIds.length > 0
	);
	const canLeavePickBan = $derived(
		(!strict || pickBanDone) && (!validateDraft || !hasDraftConflicts)
	);
	const ready = $derived(sideDone && canLeavePickBan && winner !== null);
	const serverDraft = $derived(onDraftOperation !== undefined);
	const canUseBulkInput = $derived(
		inputStep === 'PICKBAN' && sideSelected && (serverDraft || allowLocalBulkInput)
	);
	const canEditDraft = $derived(!readOnly && !busy);
	const canRecordResult = $derived(!readOnly && !busy && onSubmit !== undefined);
	const canSubmit = $derived(ready && canRecordResult);
	const progressSteps = $derived([
		{ key: 'SIDE' as const, label: '사이드', done: sideDone },
		{ key: 'PICKBAN' as const, label: '픽밴', done: pickBanDone },
		{ key: 'WINNER' as const, label: '승자', done: winner !== null }
	]);
	const canUndo = $derived(!serverDraft && canEditDraft && undoStack.length > 0);
	const canRedo = $derived(!serverDraft && canEditDraft && redoStack.length > 0);
	const sidePresets: ScrimPickBanResultInputSidePreset[] = ['DEFAULT', 'SWAPPED'];
	const winnerSides: ScrimPickBanResultInputSide[] = ['BLUE', 'RED'];
	const boardColumns: Array<'LEFT' | 'RIGHT'> = ['LEFT', 'RIGHT'];
	const pickBanTeams = $derived({
		BLUE: teamForSide('BLUE'),
		RED: teamForSide('RED')
	});
	const inputCellOrder = $derived(scrimPickBanResultInputCellOrder(pickBanTeams, bans));

	$effect(() => {
		const fingerprint = gameFingerprint(initialGame);
		if (fingerprint === lastSyncedGameFingerprint) return;

		const gameChanged = initialGame.id !== lastSyncedGameId;
		lastSyncedGameFingerprint = fingerprint;
		lastSyncedGameId = initialGame.id;
		sidePreset = initialGame.sidePreset ?? 'DEFAULT';
		if (gameChanged) {
			sideSelected = hasExplicitSideSelection(initialGame);
			bulkInputOpen = false;
		} else if (hasExplicitSideSelection(initialGame)) {
			sideSelected = true;
		}
		winner = initialGame.winner;
		bans = {
			BLUE: [...initialGame.bans.BLUE],
			RED: [...initialGame.bans.RED]
		};
		picks = { ...initialGame.picks };
		undoStack = [];
		redoStack = [];
		saved = false;
		submissionError = '';
	});

	function findDraftConflicts(
		nextPicks: Record<string, string | null>,
		nextBans: Record<ScrimPickBanResultInputSide, Array<string | null>>,
		nextHardFearless: boolean,
		priorPickChampionIds: readonly string[]
	): ScrimPickBanResultInputDraftConflicts {
		const selectedChampionIds = [
			...Object.values(nextPicks).filter((championId): championId is string => championId !== null),
			...nextBans.BLUE.filter((championId): championId is string => championId !== null),
			...nextBans.RED.filter((championId): championId is string => championId !== null)
		];
		const counts: Record<string, number> = {};
		for (const championId of selectedChampionIds) {
			counts[championId] = (counts[championId] ?? 0) + 1;
		}
		const duplicateChampionIds = Object.entries(counts).flatMap(([championId, count]) =>
			count > 1 ? [championId] : []
		);
		const priorPickIds = new Set(priorPickChampionIds);
		const hardFearlessChampionIds = nextHardFearless
			? [
					...new Set(
						Object.values(nextPicks).filter(
							(championId): championId is string =>
								championId !== null && priorPickIds.has(championId)
						)
					)
				]
			: [];

		return { duplicateChampionIds, hardFearlessChampionIds };
	}

	function hasExplicitSideSelection(game: ScrimPickBanResultInputGame): boolean {
		if (game.sideSelected !== undefined) return game.sideSelected;
		return (
			game.sidePreset === 'SWAPPED' ||
			game.winner !== null ||
			Object.values(game.picks).some(Boolean) ||
			[...game.bans.BLUE, ...game.bans.RED].some(Boolean)
		);
	}

	function gameFingerprint(game: ScrimPickBanResultInputGame): string {
		return JSON.stringify({
			id: game.id,
			sidePreset: game.sidePreset,
			sideSelected: game.sideSelected,
			winner: game.winner,
			bans: game.bans,
			picks: Object.entries(game.picks).sort(([left], [right]) => left.localeCompare(right))
		});
	}

	function captureSnapshot(): ScrimPickBanResultInputSnapshot {
		return {
			sidePreset,
			winner,
			bans: {
				BLUE: [...bans.BLUE],
				RED: [...bans.RED]
			},
			picks: { ...picks }
		};
	}

	function restoreSnapshot(snapshot: ScrimPickBanResultInputSnapshot) {
		sidePreset = snapshot.sidePreset;
		winner = snapshot.winner;
		bans = {
			BLUE: [...snapshot.bans.BLUE],
			RED: [...snapshot.bans.RED]
		};
		picks = { ...snapshot.picks };
		saved = false;
		submissionError = '';
	}

	function recordHistory() {
		undoStack = [...undoStack.slice(-39), captureSnapshot()];
		redoStack = [];
	}

	function undo() {
		if (!canUndo) return;
		const previous = undoStack.at(-1);
		if (!previous) return;
		undoStack = undoStack.slice(0, -1);
		redoStack = [...redoStack, captureSnapshot()];
		restoreSnapshot(previous);
	}

	function redo() {
		if (!canRedo) return;
		const next = redoStack.at(-1);
		if (!next) return;
		redoStack = redoStack.slice(0, -1);
		undoStack = [...undoStack, captureSnapshot()];
		restoreSnapshot(next);
	}

	async function setBan(
		side: ScrimPickBanResultInputSide,
		index: number,
		championId: string | null
	) {
		if (!canEditDraft || bans[side][index] === championId) return;
		if (onDraftOperation) {
			await onDraftOperation(
				championId
					? { type: 'SET_BAN', side, slot: index + 1, championId }
					: { type: 'CLEAR_BAN', side, slot: index + 1 }
			);
			return;
		}
		recordHistory();
		bans = {
			...bans,
			[side]: bans[side].map((value, valueIndex) => (valueIndex === index ? championId : value))
		};
		saved = false;
		submissionError = '';
	}

	async function setPick(playerId: string, championId: string | null) {
		if (!canEditDraft || (picks[playerId] ?? null) === championId) return;
		if (onDraftOperation) {
			await onDraftOperation(
				championId
					? ({ type: 'SET_PICK', playerId, championId } as SeriesPickBanDraftOperation)
					: ({ type: 'CLEAR_PICK', playerId } as SeriesPickBanDraftOperation)
			);
			return;
		}
		recordHistory();
		picks = { ...picks, [playerId]: championId };
		saved = false;
		submissionError = '';
	}

	async function applyBulkInput(items: SeriesPickBanBulkItem[]) {
		if (!canUseBulkInput || !canEditDraft || items.length === 0) return;
		if (onDraftOperation) {
			await onDraftOperation({ type: 'APPLY_BULK', items });
			return;
		}

		recordHistory();
		const nextPicks = { ...picks };
		const nextBans = { BLUE: [...bans.BLUE], RED: [...bans.RED] };
		for (const item of items) {
			if (item.type === 'SET_PICK') {
				nextPicks[item.playerId] = item.championId;
			} else {
				nextBans[item.side][item.slot - 1] = item.championId;
			}
		}
		picks = nextPicks;
		bans = nextBans;
		saved = false;
		submissionError = '';
	}

	async function selectSide(preset: ScrimPickBanResultInputSidePreset) {
		if (!canEditDraft) return;
		if (onDraftOperation) {
			if (!sideSelected || sidePreset !== preset) {
				await onDraftOperation({ type: 'SET_SIDE', sidePreset: preset });
			}
			sidePreset = preset;
			sideSelected = true;
			return;
		}
		if (sidePreset === preset) {
			sideSelected = true;
			return;
		}
		recordHistory();
		sidePreset = preset;
		sideSelected = true;
		saved = false;
		submissionError = '';
	}

	function sideForColumn(
		preset: ScrimPickBanResultInputSidePreset,
		column: 'LEFT' | 'RIGHT'
	): ScrimPickBanResultInputSide {
		if (preset === 'DEFAULT') return column === 'LEFT' ? 'BLUE' : 'RED';
		return column === 'LEFT' ? 'RED' : 'BLUE';
	}

	function sideLabel(side: ScrimPickBanResultInputSide): string {
		return side === 'BLUE' ? 'BLUE' : 'RED';
	}

	function sideClass(side: ScrimPickBanResultInputSide): string {
		return side === 'BLUE' ? 'text-blue-100' : 'text-red-100';
	}

	function sidePanelClass(side: ScrimPickBanResultInputSide): string {
		return side === 'BLUE' ? 'bg-blue-400/[0.075]' : 'bg-red-400/[0.075]';
	}

	function winnerPanelClass(selected: boolean): string {
		return selected ? 'bg-emerald-400/[0.095]' : 'bg-slate-950/20 opacity-75';
	}

	function teamForSide(side: ScrimPickBanResultInputSide): ScrimPickBanResultInputTeam {
		const source = teamsAreGameSides
			? teams[side]
			: sideForColumn(sidePreset, 'LEFT') === side
				? teams.BLUE
				: teams.RED;
		return source.side === side ? source : { ...source, side };
	}

	function setWinner(side: ScrimPickBanResultInputSide) {
		if (!canRecordResult || winner === side) return;
		recordHistory();
		winner = side;
		saved = false;
		submissionError = '';
	}

	function navigateInputCell(current: ScrimPickBanResultInputActiveCell, direction: -1 | 1) {
		if (!canEditDraft) return;
		const next = adjacentScrimPickBanResultInputCell(inputCellOrder, current, direction);
		if (!next) return;

		activeCell = next;
		if (typeof globalThis.window === 'undefined') return;
		globalThis.window.requestAnimationFrame(() => {
			const input = globalThis.document.getElementById(
				scrimPickBanResultInputCellId(next)
			) as globalThis.HTMLInputElement | null;
			input?.focus({ preventScroll: true });
			input?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
		});
	}

	function progressBarClass(done: boolean, current: boolean): string {
		if (current) return done ? 'bg-emerald-300/80' : 'bg-orange-300/80';
		return done ? 'bg-emerald-300/70' : 'bg-orange-300/60';
	}

	function progressTextClass(done: boolean, current: boolean): string {
		if (current) return done ? 'text-emerald-100' : 'text-orange-100';
		return done ? 'text-emerald-100' : 'text-orange-200/80';
	}

	function goToPickBan() {
		if (!sideDone) return;
		inputStep = 'PICKBAN';
	}

	function goToWinner() {
		if (!canLeavePickBan) return;
		inputStep = 'WINNER';
	}

	async function submit() {
		if (!canSubmit || submitting || !onSubmit) return;
		submitting = true;
		try {
			await onSubmit({
				id: initialGame.id,
				label: initialGame.label,
				sidePreset,
				winner,
				bans: { BLUE: [...bans.BLUE], RED: [...bans.RED] },
				picks: { ...picks }
			});
			saved = true;
		} catch (cause) {
			saved = false;
			submissionError = callbackErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	function callbackErrorText(cause: unknown): string {
		if (!(cause instanceof Error)) return '게임 결과를 기록하지 못했습니다.';
		const details = cause as Error & { requestId?: unknown };
		const requestId = typeof details.requestId === 'string' ? details.requestId : null;
		return requestId && !cause.message.includes(requestId)
			? `${cause.message} (요청 ID: ${requestId})`
			: cause.message;
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header class="flex min-h-10 items-center gap-2 px-3 py-1.5">
		<button
			type="button"
			class="flex min-w-0 flex-1 items-center justify-between gap-3 text-left hover:text-slate-100"
			aria-expanded={currentSetExpanded}
			aria-controls={currentSetDisclosureId}
			onclick={() => (currentSetExpanded = !currentSetExpanded)}
		>
			<span class="flex min-w-0 items-center gap-2">
				<span class="truncate text-xs font-semibold text-slate-200">현재 세트 결과</span>
				<span class="shrink-0 text-[11px] font-medium text-slate-500">{initialGame.label}</span>
			</span>
			<span
				class="inline-flex shrink-0 items-center gap-1.5 text-[11px] font-semibold text-slate-400"
			>
				{currentSetExpanded ? '접기' : '입력'}
				<ChevronDown
					size={15}
					class={`transition-transform ${currentSetExpanded ? 'rotate-180' : ''}`}
					aria-hidden="true"
				/>
			</span>
		</button>
		{#if canUseBulkInput}
			<button
				type="button"
				disabled={!canEditDraft}
				title={canEditDraft ? '일괄 입력' : '픽밴 편집 권한이 필요합니다.'}
				onclick={() => (bulkInputOpen = true)}
				class="inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-sm border border-cyan-300/25 px-2.5 text-[11px] font-semibold text-cyan-100 hover:bg-cyan-400/10 disabled:cursor-not-allowed disabled:opacity-35"
			>
				<ClipboardPenLine size={14} aria-hidden="true" /> 일괄 입력
			</button>
		{/if}
	</header>

	<div
		id={currentSetDisclosureId}
		hidden={!currentSetExpanded}
		class="grid gap-1.5 border-t border-[var(--color-app-border-muted)]"
	>
		<div class="grid grid-cols-3 gap-1 bg-[var(--color-app-surface)] px-2 py-1.5">
			{#each progressSteps as step (step.label)}
				{@const current = step.key === inputStep}
				<div class="grid gap-1">
					<div class={`h-1.5 ${progressBarClass(step.done, current)}`}></div>
					<p class={`text-center text-xs font-semibold ${progressTextClass(step.done, current)}`}>
						{step.label}
					</p>
				</div>
			{/each}
		</div>

		{#if inputStep === 'SIDE'}
			<section
				class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] p-1.5"
			>
				<div class="grid gap-1.5">
					<div class="grid grid-cols-2 gap-1">
						{#each sidePresets as preset (preset)}
							<button
								type="button"
								class={`inline-flex h-8 w-full items-center justify-center px-2.5 text-xs font-semibold transition ${
									sideSelected && sidePreset === preset
										? 'rounded-md bg-cyan-400/12 text-cyan-100'
										: 'rounded-sm bg-slate-950/25 text-slate-300 hover:bg-slate-900/55'
								}`}
								disabled={!canEditDraft}
								onclick={() => void selectSide(preset)}
							>
								{preset === 'DEFAULT' ? '왼쪽 팀 블루' : '오른쪽 팀 블루'}
							</button>
						{/each}
					</div>

					{#each sidePresets as preset (preset)}
						{#if (!sideSelected && preset === 'DEFAULT') || (sideSelected && sidePreset === preset)}
							<div
								class="grid min-w-[34rem] grid-cols-2 gap-1 overflow-x-auto bg-[var(--color-app-surface)]"
							>
								{#each boardColumns as column (column)}
									{@const side = sideForColumn(preset, column)}
									{@const team = teamsAreGameSides
										? teams[side]
										: column === 'LEFT'
											? teams.BLUE
											: teams.RED}
									<section class={sideSelected ? sidePanelClass(side) : 'bg-slate-950/20'}>
										<div class="flex h-11 items-center justify-between px-2">
											<div>
												<p class="text-sm font-semibold text-slate-100">
													{team.name}
												</p>
											</div>
											<span
												class={`inline-flex h-6 w-12 shrink-0 items-center justify-center bg-slate-950/35 text-[11px] font-semibold ${
													sideSelected ? sideClass(side) : 'invisible'
												}`}
											>
												{sideLabel(side)}
											</span>
										</div>
										<div class="grid grid-rows-[repeat(5,3rem)] gap-1 px-1.5 pb-1.5">
											{#each team.players as player (player.id)}
												<div
													class="grid min-h-0 grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-1 rounded-sm bg-slate-950/25 p-1"
												>
													<span class="grid place-items-center bg-slate-950/35 text-slate-300">
														<PositionIcon role={player.lane} size="sm" />
													</span>
													<span class="truncate text-sm font-semibold text-slate-100">
														{player.name}
													</span>
												</div>
											{/each}
										</div>
									</section>
								{/each}
							</div>
						{/if}
					{/each}
				</div>
			</section>
		{:else if inputStep === 'PICKBAN'}
			<section
				class="overflow-x-auto border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)]"
			>
				<div
					class={`grid min-w-[64rem] gap-px xl:min-w-0 ${
						canEditDraft ? '' : 'pointer-events-none opacity-60'
					}`}
					aria-busy={busy}
				>
					<ScrimPickBanResultInputTeamPanel
						team={pickBanTeams.BLUE}
						bans={bans.BLUE}
						{picks}
						{activeCell}
						{champions}
						version={patchVersion}
						onActiveCell={(cell) => {
							activeCell = cell;
						}}
						onBan={setBan}
						onPick={setPick}
						onNavigate={canEditDraft ? navigateInputCell : undefined}
					/>
					<ScrimPickBanResultInputTeamPanel
						team={pickBanTeams.RED}
						bans={bans.RED}
						{picks}
						{activeCell}
						{champions}
						version={patchVersion}
						onActiveCell={(cell) => {
							activeCell = cell;
						}}
						onBan={setBan}
						onPick={setPick}
						onNavigate={canEditDraft ? navigateInputCell : undefined}
					/>
				</div>
			</section>
			{#if validateDraft && hasDraftConflicts}
				<div
					class="border-y border-rose-300/25 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 text-rose-100"
					role="alert"
				>
					{#if draftConflicts.duplicateChampionIds.length > 0}
						<p>
							같은 세트에서 중복 선택된 챔피언: {draftConflicts.duplicateChampionIds
								.map(
									(championId) =>
										champions.find((champion) => champion.id === championId)?.name ?? championId
								)
								.join(', ')}
						</p>
					{/if}
					{#if draftConflicts.hardFearlessChampionIds.length > 0}
						<p>
							하드 피어리스에서 이전 세트에 픽한 챔피언: {draftConflicts.hardFearlessChampionIds
								.map(
									(championId) =>
										champions.find((champion) => champion.id === championId)?.name ?? championId
								)
								.join(', ')}
						</p>
					{/if}
				</div>
			{/if}
		{:else}
			<section
				class="border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] p-1.5"
			>
				<div class="grid gap-1.5">
					<div class="grid grid-cols-2 gap-1">
						{#each winnerSides as side (side)}
							{@const team = teamForSide(side)}
							<button
								type="button"
								class={`inline-flex h-8 w-full items-center justify-center px-2.5 text-xs font-semibold transition ${
									winner === side
										? 'rounded-md bg-emerald-400/14 text-emerald-100'
										: 'rounded-sm bg-slate-950/25 text-slate-300 hover:bg-slate-900/55'
								}`}
								disabled={!canRecordResult}
								onclick={() => setWinner(side)}
							>
								{sideLabel(side)} 승리 · {team.name}
							</button>
						{/each}
					</div>

					<div
						class="grid min-w-[34rem] grid-cols-2 gap-1 overflow-x-auto bg-[var(--color-app-surface)]"
					>
						{#each winnerSides as side (side)}
							{@const team = teamForSide(side)}
							{@const selected = winner === side}
							<section class={winnerPanelClass(selected)}>
								<div class="flex h-11 items-center justify-between px-2">
									<p class={`text-sm font-semibold ${sideClass(side)}`}>{sideLabel(side)}</p>
									<div class="flex items-center gap-1">
										<span
											class="bg-slate-950/35 px-2 py-1 text-[11px] font-semibold text-slate-100"
										>
											{team.name}
										</span>
										{#if selected}
											<span
												class="bg-emerald-400/14 px-2 py-1 text-[11px] font-semibold text-emerald-100"
											>
												승리
											</span>
										{/if}
									</div>
								</div>
								<div class="grid grid-rows-[repeat(5,3rem)] gap-1 px-1.5 pb-1.5">
									{#each team.players as player (player.id)}
										<div
											class="grid min-h-0 grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-1 rounded-sm bg-slate-950/25 p-1"
										>
											<span class="grid place-items-center bg-slate-950/35 text-slate-300">
												<PositionIcon role={player.lane} size="sm" />
											</span>
											<span class="truncate text-sm font-semibold text-slate-100">
												{player.name}
											</span>
										</div>
									{/each}
								</div>
							</section>
						{/each}
					</div>
				</div>
			</section>
		{/if}

		<div class="bg-[var(--color-app-surface)] p-1.5">
			<div class="flex flex-wrap items-center justify-end gap-1.5">
				{#if !serverDraft}
					<div class="flex items-center gap-1">
						<button
							type="button"
							class="grid size-8 place-items-center rounded-sm text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-35"
							disabled={!canUndo}
							aria-label="되돌리기"
							title="되돌리기"
							onclick={undo}
						>
							<Undo2 size={15} />
						</button>
						<button
							type="button"
							class="grid size-8 place-items-center rounded-sm text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-35"
							disabled={!canRedo}
							aria-label="다시 실행"
							title="다시 실행"
							onclick={redo}
						>
							<Redo2 size={15} />
						</button>
					</div>
				{/if}
				{#if inputStep === 'SIDE'}
					<button
						type="button"
						class="inline-flex h-9 items-center justify-center rounded-sm bg-cyan-400/12 px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-35"
						disabled={!sideDone}
						onclick={goToPickBan}
					>
						다음
					</button>
				{:else if inputStep === 'PICKBAN'}
					<button
						type="button"
						class="inline-flex h-9 items-center justify-center rounded-sm px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
						onclick={() => {
							inputStep = 'SIDE';
						}}
					>
						이전
					</button>
					<button
						type="button"
						class="inline-flex h-9 items-center justify-center rounded-sm bg-cyan-400/12 px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-50"
						disabled={!canLeavePickBan}
						onclick={goToWinner}
					>
						다음
					</button>
				{:else if inputStep === 'WINNER'}
					<button
						type="button"
						class="inline-flex h-9 items-center justify-center rounded-sm px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
						onclick={goToPickBan}
					>
						이전
					</button>
					<button
						type="button"
						class="inline-flex h-9 items-center justify-center rounded-sm bg-emerald-400/14 px-4 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-50"
						disabled={!canSubmit || submitting}
						title={onSubmit ? undefined : '결과 기록 API가 연결되면 사용할 수 있습니다.'}
						onclick={submit}
					>
						{submitting ? '기록 중' : '결과 기록'}
					</button>
				{/if}
			</div>

			{#if saved}
				<div class="bg-emerald-400/10 px-3 py-2 text-xs font-semibold text-emerald-100">
					결과가 기록되었습니다.
				</div>
			{/if}
			{#if submissionError}
				<p
					class="border border-rose-300/25 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 text-rose-100"
					role="alert"
				>
					{submissionError}
				</p>
			{/if}
		</div>

		{#if canUseBulkInput}
			<ScrimPickBanBulkInput
				bind:open={bulkInputOpen}
				teams={pickBanTeams}
				{picks}
				{bans}
				{champions}
				{hardFearless}
				{previousPickChampionIds}
				readOnly={!canEditDraft}
				{busy}
				onApply={applyBulkInput}
			/>
		{/if}
	</div>
</section>
