<script lang="ts">
	import { Redo2, Undo2 } from 'lucide-svelte';
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
	import PositionIcon from './PositionIcon.svelte';
	import type { ScrimPickBanResultInputActiveCell } from './scrimPickBanResultInputHelpers';
	import ScrimPickBanResultInputTeamPanel from './ScrimPickBanResultInputTeamPanel.svelte';

	interface Props {
		strict?: boolean;
		teams?: Record<ScrimPickBanResultInputSide, ScrimPickBanResultInputTeam>;
		initialGame?: ScrimPickBanResultInputGame;
		champions?: ScrimPickBanResultInputChampion[];
		patchVersion?: string;
		onSubmit?: (game: ScrimPickBanResultInputGame) => void | Promise<void>;
	}

	let {
		strict = false,
		teams = mockScrimPickBanResultInputTeams,
		initialGame = mockScrimPickBanResultInputGames[0]!,
		champions = scrimPickBanResultInputChampions,
		patchVersion = mockScrimPickBanResultInputPatch,
		onSubmit
	}: Props = $props();

	type ScrimPickBanResultInputStep = 'SIDE' | 'PICKBAN' | 'WINNER';
	type ScrimPickBanResultInputSnapshot = {
		sidePreset: ScrimPickBanResultInputSidePreset;
		winner: ScrimPickBanResultInputSide | null;
		bans: Record<ScrimPickBanResultInputSide, Array<string | null>>;
		picks: Record<string, string | null>;
	};

	let inputStep = $state<ScrimPickBanResultInputStep>('SIDE');
	let sidePreset = $state<ScrimPickBanResultInputSidePreset>(
		untrack(() => initialGame.sidePreset ?? 'DEFAULT')
	);
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

	const allBansFilled = $derived([...bans.BLUE, ...bans.RED].every(Boolean));
	const allPicksFilled = $derived(Object.values(picks).every(Boolean));
	const sideDone = $derived(true);
	const pickBanDone = $derived(allBansFilled && allPicksFilled);
	const canLeavePickBan = $derived(!strict || pickBanDone);
	const ready = $derived(sideDone && canLeavePickBan && winner !== null);
	const progressSteps = $derived([
		{ key: 'SIDE' as const, label: '사이드', done: sideDone },
		{ key: 'PICKBAN' as const, label: '픽밴', done: pickBanDone },
		{ key: 'WINNER' as const, label: '승자', done: winner !== null }
	]);
	const canUndo = $derived(undoStack.length > 0);
	const canRedo = $derived(redoStack.length > 0);
	const sidePresets: ScrimPickBanResultInputSidePreset[] = ['DEFAULT', 'SWAPPED'];
	const winnerSides: ScrimPickBanResultInputSide[] = ['BLUE', 'RED'];

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

	function setBan(side: ScrimPickBanResultInputSide, index: number, championId: string | null) {
		if (bans[side][index] === championId) return;
		recordHistory();
		bans = {
			...bans,
			[side]: bans[side].map((value, valueIndex) => (valueIndex === index ? championId : value))
		};
		saved = false;
	}

	function setPick(playerId: string, championId: string | null) {
		if ((picks[playerId] ?? null) === championId) return;
		recordHistory();
		picks = { ...picks, [playerId]: championId };
		saved = false;
	}

	function selectSide(preset: ScrimPickBanResultInputSidePreset) {
		if (sidePreset === preset) return;
		recordHistory();
		sidePreset = preset;
		saved = false;
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

	function teamForSide(side: ScrimPickBanResultInputSide) {
		return sideForColumn(sidePreset, 'LEFT') === side ? teams.BLUE : teams.RED;
	}

	function setWinner(side: ScrimPickBanResultInputSide) {
		if (winner === side) return;
		recordHistory();
		winner = side;
		saved = false;
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
		inputStep = 'PICKBAN';
	}

	function goToWinner() {
		if (!canLeavePickBan) return;
		inputStep = 'WINNER';
	}

	async function submit() {
		if (!ready || submitting) return;
		submitting = true;
		try {
			await onSubmit?.({
				id: initialGame.id,
				label: initialGame.label,
				sidePreset,
				winner,
				bans: { BLUE: [...bans.BLUE], RED: [...bans.RED] },
				picks: { ...picks }
			});
			saved = true;
		} finally {
			submitting = false;
		}
	}
</script>

<div class="grid gap-1.5">
	<div
		class="grid gap-1.5 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] px-3 py-1.5 md:grid-cols-[minmax(0,1fr)_auto] md:items-center"
	>
		<div class="min-w-0">
			<p class="text-xs font-semibold text-slate-500">결과 입력</p>
		</div>
		<div class="flex flex-wrap items-center justify-end gap-1.5">
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
		</div>
	</div>

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
								sidePreset === preset
									? 'rounded-md bg-cyan-400/12 text-cyan-100'
									: 'rounded-sm bg-slate-950/25 text-slate-300 hover:bg-slate-900/55'
							}`}
							onclick={() => selectSide(preset)}
						>
							{preset === 'DEFAULT' ? '왼쪽 팀 블루' : '오른쪽 팀 블루'}
						</button>
					{/each}
				</div>

				{#each sidePresets as preset (preset)}
					{#if sidePreset === preset}
						<div
							class="grid min-w-[34rem] grid-cols-2 gap-1 overflow-x-auto bg-[var(--color-app-surface)]"
						>
							{#each [{ column: 'LEFT' as const, team: teams.BLUE }, { column: 'RIGHT' as const, team: teams.RED }] as item (item.column)}
								<section class={sidePanelClass(sideForColumn(preset, item.column))}>
									<div class="flex h-11 items-center justify-between px-2">
										<div>
											<p class="text-sm font-semibold text-slate-100">
												{item.team.name}
											</p>
										</div>
										<span
											class={`bg-slate-950/35 px-2 py-1 text-[11px] font-semibold ${sideClass(sideForColumn(preset, item.column))}`}
										>
											{sideLabel(sideForColumn(preset, item.column))}
										</span>
									</div>
									<div class="grid grid-rows-[repeat(5,3rem)] gap-1 px-1.5 pb-1.5">
										{#each item.team.players as player (player.id)}
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
			<div class="grid min-w-[64rem] gap-px xl:min-w-0">
				<ScrimPickBanResultInputTeamPanel
					team={teams.BLUE}
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
				/>
				<ScrimPickBanResultInputTeamPanel
					team={teams.RED}
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
				/>
			</div>
		</section>
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
									<span class="bg-slate-950/35 px-2 py-1 text-[11px] font-semibold text-slate-100">
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
		<div class="flex flex-wrap justify-end gap-1.5">
			{#if inputStep === 'SIDE'}
				<button
					type="button"
					class="inline-flex h-9 items-center justify-center rounded-sm bg-cyan-400/12 px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/18"
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
					disabled={!ready || submitting}
					onclick={submit}
				>
					{submitting ? '기록 중' : '결과 기록'}
				</button>
			{/if}
		</div>
	</div>

	{#if saved}
		<div class="bg-emerald-400/10 px-3 py-2 text-xs font-semibold text-emerald-100">
			결과가 기록되었습니다.
		</div>
	{/if}
</div>
