<script lang="ts">
	import { Plus, RefreshCcw, Route, X } from 'lucide-svelte';

	interface LadderItem {
		id: string;
		label: string;
	}

	interface LadderBridge {
		row: number;
		left: number;
	}

	interface LadderTrace {
		input: number;
		output: number;
		entryId: string;
		d: string;
		color: string;
	}

	const initialEntries: LadderItem[] = [
		{ id: 'entry-blue', label: '1팀' },
		{ id: 'entry-red', label: '2팀' },
		{ id: 'entry-observer', label: '관전' }
	];

	const initialResults: LadderItem[] = [
		{ id: 'result-blue', label: '블루' },
		{ id: 'result-red', label: '레드' },
		{ id: 'result-wait', label: '대기' }
	];

	const tracePalette = ['#22d3ee', '#fbbf24', '#a78bfa', '#34d399', '#fb7185', '#60a5fa'];

	let entries = $state<LadderItem[]>(initialEntries);
	let results = $state<LadderItem[]>(initialResults);
	let entryInput = $state('');
	let resultInput = $state('');
	let bridges = $state<LadderBridge[]>([]);
	let traces = $state<LadderTrace[]>([]);
	let activeEntryIds = $state<string[]>([]);
	let doneEntryIds = $state<string[]>([]);
	let selectedEntryId = $state<string | null>(null);
	let ladderRunning = $state(false);
	let ladderDone = $state(false);
	let playCount = $state(0);
	let runId = $state(0);
	let nextId = $state(1);
	let timers: Array<ReturnType<typeof globalThis.setTimeout>> = [];
	let animationFrames: Array<ReturnType<typeof globalThis.requestAnimationFrame>> = [];

	const rowCount = $derived(Math.max(8, entries.length * 4));
	const canPlay = $derived(entries.length >= 2 && entries.length === results.length);
	const selectedTrace = $derived(
		selectedEntryId ? (traces.find((trace) => trace.entryId === selectedEntryId) ?? null) : null
	);

	function xFor(index: number): number {
		if (entries.length <= 1) return 360;
		return 60 + (index * 600) / (entries.length - 1);
	}

	function yFor(row: number): number {
		return 54 + (row * 312) / Math.max(rowCount - 1, 1);
	}

	function randomInt(max: number): number {
		const buffer = new Uint32Array(1);
		globalThis.crypto.getRandomValues(buffer);
		return buffer[0] % max;
	}

	function clearTimers() {
		for (const timer of timers) globalThis.clearTimeout(timer);
		timers = [];
		for (const frame of animationFrames) globalThis.cancelAnimationFrame(frame);
		animationFrames = [];
	}

	function requestNextFrame(callback: () => void) {
		const frame = globalThis.requestAnimationFrame(() => {
			animationFrames = animationFrames.filter((queuedFrame) => queuedFrame !== frame);
			callback();
		});
		animationFrames.push(frame);
	}

	function scheduleAfterPaint(callback: () => void, delayMs: number) {
		timers.push(
			globalThis.setTimeout(() => {
				requestNextFrame(() => requestNextFrame(callback));
			}, delayMs)
		);
	}

	function resetLadder() {
		clearTimers();
		bridges = [];
		traces = [];
		activeEntryIds = [];
		doneEntryIds = [];
		selectedEntryId = null;
		ladderRunning = false;
		ladderDone = false;
		runId += 1;
	}

	function addEntry() {
		const label = entryInput.trim();
		if (!label || entries.length >= 6) return;
		entries = [...entries, { id: `entry-custom-${nextId}`, label }];
		results = [...results, { id: `result-custom-${nextId}`, label }];
		nextId += 1;
		entryInput = '';
		resetLadder();
	}

	function addResult() {
		const label = resultInput.trim();
		if (!label || results.length >= 6) return;
		results = [...results, { id: `result-extra-${nextId}`, label }];
		nextId += 1;
		resultInput = '';
		resetLadder();
	}

	function removeEntry(id: string) {
		if (entries.length <= 2 || ladderRunning) return;
		entries = entries.filter((entry) => entry.id !== id);
		resetLadder();
	}

	function removeResult(id: string) {
		if (results.length <= 2 || ladderRunning) return;
		results = results.filter((result) => result.id !== id);
		resetLadder();
	}

	function generateBridges(): LadderBridge[] {
		const next: LadderBridge[] = [];
		for (let row = 1; row < rowCount - 1; row += 1) {
			if (randomInt(100) >= 72) continue;
			const left = randomInt(entries.length - 1);
			const previous = next.at(-1);
			if (previous && previous.row === row - 1 && Math.abs(previous.left - left) <= 1) continue;
			next.push({ row, left });
		}
		return next;
	}

	function buildTrace(input: number, nextBridges: LadderBridge[]): LadderTrace {
		let col = input;
		let d = `M ${xFor(col)} 36`;
		const sortedBridges = [...nextBridges].sort((a, b) => a.row - b.row);

		for (const bridge of sortedBridges) {
			const y = yFor(bridge.row);
			if (bridge.left === col) {
				d += ` L ${xFor(col)} ${y} L ${xFor(col + 1)} ${y}`;
				col += 1;
			} else if (bridge.left + 1 === col) {
				d += ` L ${xFor(col)} ${y} L ${xFor(col - 1)} ${y}`;
				col -= 1;
			}
		}

		d += ` L ${xFor(col)} 384`;
		return {
			input,
			output: col,
			entryId: entries[input].id,
			d,
			color: tracePalette[input % tracePalette.length]
		};
	}

	function playLadder() {
		if (!canPlay || ladderRunning) return;

		clearTimers();
		const nextBridges = generateBridges();
		const nextTraces = entries.map((_, index) => buildTrace(index, nextBridges));

		bridges = nextBridges;
		traces = nextTraces;
		activeEntryIds = [];
		doneEntryIds = [];
		selectedEntryId = null;
		ladderRunning = true;
		ladderDone = false;
		playCount += 1;
		runId += 1;

		nextTraces.forEach((trace, index) => {
			scheduleAfterPaint(() => {
				activeEntryIds = [...activeEntryIds, trace.entryId];
				timers.push(
					globalThis.setTimeout(() => {
						activeEntryIds = activeEntryIds.filter((id) => id !== trace.entryId);
						doneEntryIds = [...doneEntryIds, trace.entryId];
						selectedEntryId = trace.entryId;
						if (doneEntryIds.length >= nextTraces.length) {
							ladderRunning = false;
							ladderDone = true;
						}
					}, 1320)
				);
			}, index * 260);
		});
	}

	function traceVisible(trace: LadderTrace): boolean {
		return activeEntryIds.includes(trace.entryId) || doneEntryIds.includes(trace.entryId);
	}

	function traceActive(trace: LadderTrace): boolean {
		return activeEntryIds.includes(trace.entryId);
	}
</script>

<div class="grid gap-4 p-4">
	<div class="grid gap-3 bg-[var(--color-app-bg-soft)] p-4">
		<div class="flex items-center justify-between gap-3">
			<div class="flex items-center gap-2">
				<Route size={17} class="text-cyan-100" />
				<p class="text-sm font-semibold text-slate-100">사다리</p>
			</div>
			<p class="text-xs font-semibold text-slate-500">{playCount}회</p>
		</div>

		<div class="ladder-stage">
			<div class="ladder-label-row">
				{#each entries as entry, index (entry.id)}
					<button
						type="button"
						class={`ladder-label-chip ${selectedEntryId === entry.id ? 'active' : ''}`}
						style={`left: ${(xFor(index) / 720) * 100}%; --trace-color: ${tracePalette[index % tracePalette.length]};`}
						onclick={() => {
							if (traces.length > 0) selectedEntryId = entry.id;
						}}
					>
						<span>{entry.label}</span>
					</button>
				{/each}
			</div>

			<svg class="ladder-board" viewBox="0 0 720 420" role="img" aria-label="사다리">
				<defs>
					<linearGradient id="ladder-post-gradient" x1="0" x2="1">
						<stop offset="0%" stop-color="rgba(148, 163, 184, 0.28)" />
						<stop offset="50%" stop-color="rgba(226, 232, 240, 0.72)" />
						<stop offset="100%" stop-color="rgba(148, 163, 184, 0.28)" />
					</linearGradient>
				</defs>

				{#each entries as entry, index (entry.id)}
					<line
						class="ladder-post"
						x1={xFor(index)}
						y1="36"
						x2={xFor(index)}
						y2="384"
						vector-effect="non-scaling-stroke"
					/>
					<circle
						class={`ladder-output-pin fab-minigame-ladder-output-pin ${selectedTrace?.output === index ? 'active' : ''}`}
						cx={xFor(index)}
						cy="384"
						r={ladderDone ? 8 : 5}
					/>
				{/each}

				{#each bridges as bridge (`bridge-${runId}-${bridge.row}-${bridge.left}`)}
					<line
						class={`ladder-bridge fab-minigame-ladder-bridge ${ladderRunning || ladderDone ? 'revealed' : ''}`}
						x1={xFor(bridge.left)}
						y1={yFor(bridge.row)}
						x2={xFor(bridge.left + 1)}
						y2={yFor(bridge.row)}
						vector-effect="non-scaling-stroke"
						style={`transition-delay: ${bridge.row * 28}ms`}
					/>
				{/each}

				{#each traces as trace (`trace-${runId}-${trace.entryId}`)}
					{@const visible = traceVisible(trace)}
					<path
						class={`ladder-trace fab-minigame-ladder-trace ${visible ? 'active' : ''}`}
						d={trace.d}
						stroke={trace.color}
						pathLength="100"
					/>
					{#if traceActive(trace)}
						<circle class="ladder-dot fab-minigame-ladder-dot active" r="7" fill={trace.color}>
							<animateMotion dur="1.25s" fill="freeze" begin="0s" path={trace.d} />
						</circle>
					{/if}
				{/each}
			</svg>

			<div class="ladder-label-row ladder-output-row">
				{#each results as result, index (result.id)}
					<div
						class={`ladder-label-chip output ${selectedTrace?.output === index ? 'active' : ''}`}
						style={`left: ${(xFor(index) / 720) * 100}%;`}
					>
						<span>{result.label}</span>
					</div>
				{/each}
			</div>
		</div>

		{#if doneEntryIds.length > 0}
			<div class="grid gap-1 bg-slate-900/70 p-2 sm:grid-cols-2">
				{#each traces as trace (trace.entryId)}
					{#if doneEntryIds.includes(trace.entryId)}
						<button
							type="button"
							class={`grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2 px-2 py-1.5 text-xs font-semibold ${
								selectedEntryId === trace.entryId
									? 'rounded-md bg-cyan-400/12 text-cyan-50'
									: 'rounded-sm text-slate-400 hover:bg-slate-800/70 hover:text-slate-100'
							}`}
							onclick={() => {
								selectedEntryId = trace.entryId;
							}}
						>
							<span class="truncate text-left">{entries[trace.input]?.label}</span>
							<span class="text-slate-500">→</span>
							<span class="truncate text-right">{results[trace.output]?.label}</span>
						</button>
					{/if}
				{/each}
			</div>
		{/if}
	</div>

	<div class="grid gap-2 sm:grid-cols-2">
		<section class="grid gap-2 bg-[var(--color-app-bg-soft)] p-3">
			<div class="flex items-center justify-between gap-3">
				<p class="text-sm font-semibold text-slate-100">입력</p>
				<p class="text-xs font-semibold text-slate-500">{entries.length}개</p>
			</div>
			<div class="grid max-h-24 gap-1 overflow-auto">
				{#each entries as entry (entry.id)}
					<div
						class="grid h-8 min-w-0 grid-cols-[minmax(0,1fr)_1rem] items-center gap-2 overflow-hidden rounded-sm bg-slate-900/70 px-2 text-[11px] font-semibold text-slate-400"
					>
						<span class="truncate">{entry.label}</span>
						<button
							type="button"
							class="grid size-4 place-items-center justify-self-end opacity-60 hover:text-slate-100"
							aria-label={`${entry.label} 제거`}
							disabled={ladderRunning || entries.length <= 2}
							onclick={() => removeEntry(entry.id)}
						>
							<X size={12} />
						</button>
					</div>
				{/each}
			</div>
			<form
				class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]"
				onsubmit={(event) => {
					event.preventDefault();
					addEntry();
				}}
			>
				<input
					bind:value={entryInput}
					aria-label="입력 항목"
					placeholder="입력 항목"
					class="min-w-0 rounded-sm bg-[#08131d] px-3 py-2.5 text-sm font-medium text-slate-50 outline-none placeholder:text-slate-500"
					disabled={ladderRunning || entries.length >= 6}
				/>
				<button
					type="submit"
					class="inline-flex h-10 items-center justify-center gap-1.5 rounded-sm bg-cyan-400/12 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-50"
					disabled={!entryInput.trim() || ladderRunning || entries.length >= 6}
				>
					<Plus size={14} />
					입력 추가
				</button>
			</form>
		</section>

		<section class="grid gap-2 bg-[var(--color-app-bg-soft)] p-3">
			<div class="flex items-center justify-between gap-3">
				<p class="text-sm font-semibold text-slate-100">결과</p>
				<p class="text-xs font-semibold text-slate-500">{results.length}개</p>
			</div>
			<div class="grid max-h-24 gap-1 overflow-auto">
				{#each results as result (result.id)}
					<div
						class="grid h-8 min-w-0 grid-cols-[minmax(0,1fr)_1rem] items-center gap-2 overflow-hidden rounded-sm bg-slate-900/70 px-2 text-[11px] font-semibold text-slate-400"
					>
						<span class="truncate">{result.label}</span>
						<button
							type="button"
							class="grid size-4 place-items-center justify-self-end opacity-60 hover:text-slate-100"
							aria-label={`${result.label} 제거`}
							disabled={ladderRunning || results.length <= 2}
							onclick={() => removeResult(result.id)}
						>
							<X size={12} />
						</button>
					</div>
				{/each}
			</div>
			<form
				class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]"
				onsubmit={(event) => {
					event.preventDefault();
					addResult();
				}}
			>
				<input
					bind:value={resultInput}
					aria-label="결과 이름"
					placeholder="결과 입력"
					class="min-w-0 rounded-sm bg-[#08131d] px-3 py-2.5 text-sm font-medium text-slate-50 outline-none placeholder:text-slate-500"
					disabled={ladderRunning || results.length >= 6}
				/>
				<button
					type="submit"
					class="inline-flex h-10 items-center justify-center gap-1.5 rounded-sm bg-cyan-400/12 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-50"
					disabled={!resultInput.trim() || ladderRunning || results.length >= 6}
				>
					<Plus size={14} />
					결과 추가
				</button>
			</form>
		</section>
	</div>

	<div class="grid grid-cols-[1fr_auto] gap-2">
		<button
			type="button"
			class="inline-flex h-10 items-center justify-center gap-2 rounded-sm bg-cyan-400/12 px-4 text-sm font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-60"
			disabled={!canPlay || ladderRunning}
			onclick={playLadder}
		>
			<Route size={16} />
			{ladderRunning
				? '진행 중'
				: ladderDone
					? '다시 타기'
					: canPlay
						? '사다리 타기'
						: '개수 맞추기'}
		</button>
		<button
			type="button"
			class="grid size-10 place-items-center rounded-sm text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
			aria-label="사다리 초기화"
			onclick={resetLadder}
		>
			<RefreshCcw size={16} />
		</button>
	</div>
</div>

<style>
	.ladder-stage {
		overflow: visible;
		border: 1px solid var(--color-app-border-muted);
		background:
			radial-gradient(circle at 50% 0%, rgb(34 211 238 / 0.12), transparent 36%),
			linear-gradient(rgb(148 163 184 / 0.06) 1px, transparent 1px),
			linear-gradient(90deg, rgb(148 163 184 / 0.06) 1px, transparent 1px), rgb(2 6 23 / 0.38);
		background-size:
			auto,
			24px 24px,
			24px 24px,
			auto;
		padding: 0.75rem;
	}

	.ladder-label-row {
		position: relative;
		min-height: 1.75rem;
	}

	.ladder-label-chip {
		position: absolute;
		top: 0;
		max-width: 5.75rem;
		transform: translateX(-50%);
		border: 1px solid var(--color-app-border-muted);
		background: rgb(15 23 42 / 0.82);
		padding: 0.32rem 0.45rem;
		text-align: center;
		font-size: 0.72rem;
		font-weight: 700;
		color: rgb(203 213 225);
		white-space: nowrap;
	}

	.ladder-label-chip.active {
		border-color: color-mix(in oklch, var(--trace-color, #67e8f9), transparent 30%);
		background: color-mix(in oklch, var(--trace-color, #67e8f9), transparent 86%);
		color: rgb(240 249 255);
	}

	.ladder-label-chip.output.active {
		border-color: rgb(103 232 249 / 0.45);
		background: rgb(34 211 238 / 0.12);
	}

	.ladder-label-chip > span {
		display: block;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.ladder-board {
		display: block;
		width: 100%;
		height: auto;
		min-height: 14rem;
		filter: drop-shadow(0 16px 14px rgb(0 0 0 / 0.22));
		transform: rotateX(6deg);
		transform-origin: 50% 0%;
	}

	.ladder-post {
		stroke: url(#ladder-post-gradient);
		stroke-width: 7;
		stroke-linecap: round;
		filter: drop-shadow(0 0 5px rgb(226 232 240 / 0.14));
	}

	.ladder-bridge {
		stroke: rgb(103 232 249 / 0.62);
		stroke-width: 4;
		stroke-linecap: round;
		opacity: 0;
		transition: opacity 240ms ease-out;
	}

	.ladder-bridge.revealed {
		opacity: 1;
		animation: ladder-bridge-flash 520ms ease-out;
	}

	.ladder-trace {
		fill: none;
		stroke-width: 7;
		stroke-linecap: round;
		stroke-linejoin: round;
		stroke-dasharray: 100;
		stroke-dashoffset: 100;
		visibility: hidden;
		transition: stroke-dashoffset 1250ms cubic-bezier(0.45, 0, 0.55, 1);
	}

	.ladder-trace.active {
		visibility: visible;
		stroke-dashoffset: 0;
	}

	.ladder-dot {
		visibility: hidden;
		stroke: white;
		stroke-width: 2;
		filter: drop-shadow(0 0 6px rgb(255 255 255 / 0.32));
	}

	.ladder-dot.active {
		visibility: visible;
	}

	.ladder-output-pin {
		fill: rgb(15 23 42);
		stroke: rgb(226 232 240 / 0.62);
		stroke-width: 3;
		transition: r 200ms;
	}

	.ladder-output-pin.active {
		fill: rgb(34 211 238 / 0.24);
		stroke: rgb(103 232 249);
	}

	@keyframes ladder-bridge-flash {
		0% {
			stroke-width: 7;
			opacity: 0.72;
		}
		100% {
			stroke-width: 4;
			opacity: 1;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.ladder-bridge,
		.ladder-trace {
			transition-duration: 1ms !important;
		}

		.ladder-bridge.revealed {
			animation: none !important;
		}
	}
</style>
