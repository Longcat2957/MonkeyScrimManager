<script lang="ts">
	import { Disc3, Plus, X } from 'lucide-svelte';

	interface RouletteOption {
		id: string;
		label: string;
		tone: string;
	}

	const initialOptions: RouletteOption[] = [
		{ id: 'top', label: '탑', tone: '#22d3ee' },
		{ id: 'jungle', label: '정글', tone: '#34d399' },
		{ id: 'mid', label: '미드', tone: '#fbbf24' },
		{ id: 'bottom', label: '원딜', tone: '#fb7185' },
		{ id: 'support', label: '서폿', tone: '#a78bfa' }
	];

	const tones = ['#22d3ee', '#34d399', '#fbbf24', '#fb7185', '#a78bfa', '#60a5fa', '#f472b6'];

	let options = $state<RouletteOption[]>(initialOptions);
	let inputValue = $state('');
	let result = $state<RouletteOption | null>(null);
	let pendingResult = $state<RouletteOption | null>(null);
	let spinCount = $state(0);
	let spinning = $state(false);
	let rotation = $state(0);
	let nextOptionId = $state(initialOptions.length + 1);

	const canSpin = $derived(options.length >= 2);
	const segmentSize = $derived(options.length > 0 ? 360 / options.length : 360);

	function addOption() {
		const label = inputValue.trim();
		if (!label || spinning || options.length >= 12) return;
		if (options.some((option) => option.label.toLowerCase() === label.toLowerCase())) {
			inputValue = '';
			return;
		}

		options = [
			...options,
			{
				id: `custom-${nextOptionId}`,
				label,
				tone: tones[nextOptionId % tones.length]
			}
		];
		nextOptionId += 1;
		inputValue = '';
	}

	function removeOption(optionId: string) {
		if (spinning) return;
		options = options.filter((option) => option.id !== optionId);
		if (result?.id === optionId) result = null;
		if (pendingResult?.id === optionId) pendingResult = null;
	}

	function randomIndex(): number {
		const buffer = new Uint32Array(1);
		globalThis.crypto.getRandomValues(buffer);
		return buffer[0] % options.length;
	}

	function normalizeDegrees(value: number): number {
		return ((value % 360) + 360) % 360;
	}

	function spinRoulette() {
		if (spinning || !canSpin) return;

		const index = randomIndex();
		const selected = options[index];
		const selectedCenter = index * segmentSize + segmentSize / 2;
		const fullTurns = 4 + (spinCount % 2);
		const currentRotation = normalizeDegrees(rotation);
		const targetRotation = normalizeDegrees(360 - selectedCenter);
		const rotationDelta = normalizeDegrees(targetRotation - currentRotation);

		result = null;
		pendingResult = selected;
		spinCount += 1;
		spinning = true;
		rotation += fullTurns * 360 + rotationDelta;

		globalThis.setTimeout(() => {
			result = pendingResult;
			pendingResult = null;
			spinning = false;
		}, 900);
	}

	function optionStyle(option: RouletteOption): string {
		return `--roulette-tone: ${option.tone};`;
	}

	function polarPoint(angle: number, radius: number): { x: number; y: number } {
		const radians = (angle * Math.PI) / 180;
		return {
			x: 50 + radius * Math.cos(radians),
			y: 50 + radius * Math.sin(radians)
		};
	}

	function segmentPath(index: number): string {
		const startAngle = index * segmentSize - 90;
		const endAngle = (index + 1) * segmentSize - 90;
		const start = polarPoint(startAngle, 48);
		const end = polarPoint(endAngle, 48);
		const largeArc = segmentSize > 180 ? 1 : 0;

		return `M 50 50 L ${start.x} ${start.y} A 48 48 0 ${largeArc} 1 ${end.x} ${end.y} Z`;
	}

	function markerPoint(index: number): { x: number; y: number } {
		return polarPoint(index * segmentSize + segmentSize / 2 - 90, 32);
	}
</script>

<div class="grid gap-4 p-4">
	<div class="grid gap-3 bg-[var(--color-app-bg-soft)] p-4">
		<div class="flex items-center justify-between gap-3">
			<div class="flex items-center gap-2">
				<Disc3 size={17} class="text-cyan-100" />
				<p class="text-sm font-semibold text-slate-100">룰렛</p>
			</div>
			<p class="text-xs font-semibold text-slate-500">{spinCount}회</p>
		</div>

		<div class="grid place-items-center py-4">
			<div class="relative grid size-36 place-items-center">
				<div
					class="roulette-pointer absolute -top-1 z-10 h-0 w-0 border-x-[7px] border-t-[12px] border-x-transparent border-t-cyan-100"
				></div>
				<div
					class="roulette-wheel fab-minigame-roulette-wheel size-32 overflow-hidden rounded-full border border-slate-700 shadow-xl shadow-black/30"
					style={`transform: rotate(${rotation}deg);`}
					aria-hidden="true"
				>
					<svg class="size-full" viewBox="0 0 100 100">
						{#each options as option, index (option.id)}
							{@const point = markerPoint(index)}
							<path
								d={segmentPath(index)}
								fill={option.tone}
								stroke="rgb(15 23 42 / 0.55)"
								stroke-width="0.8"
							/>
							<circle
								cx={point.x}
								cy={point.y}
								r="7"
								fill={option.tone}
								stroke="rgb(15 23 42 / 0.72)"
								stroke-width="1.4"
							/>
							<text
								x={point.x}
								y={point.y}
								text-anchor="middle"
								dominant-baseline="central"
								class="fill-slate-950 text-[10px] font-black"
							>
								{index + 1}
							</text>
						{/each}
					</svg>
				</div>
				{#if result}
					<div
						class="absolute grid size-16 place-items-center rounded-full border border-slate-700 bg-slate-950/85 text-center shadow-lg shadow-black/35"
						aria-live="polite"
					>
						<div class="min-w-0 px-1">
							<p class="truncate text-xl font-black leading-none tracking-normal text-slate-50">
								{result.label}
							</p>
							<p class="mt-1 text-[11px] font-semibold text-slate-500">결과</p>
						</div>
					</div>
				{/if}
			</div>
		</div>

		<div class="grid max-h-24 grid-cols-2 gap-1 overflow-auto sm:grid-cols-3">
			{#each options as option, index (option.id)}
				<button
					type="button"
					class={`grid h-9 min-w-0 grid-cols-[minmax(0,1fr)_1rem] items-center gap-2 overflow-hidden px-2.5 text-left text-[11px] font-semibold ${
						result?.id === option.id
							? 'rounded-md bg-cyan-400/12 text-cyan-50'
							: 'rounded-sm bg-slate-900/70 text-slate-400'
					}`}
					disabled={spinning}
					onclick={() => removeOption(option.id)}
					aria-label={`${option.label} 제거`}
				>
					<span class="flex min-w-0 items-center gap-1.5 overflow-hidden pr-1">
						<span
							class="roulette-chip-number grid size-4 shrink-0 place-items-center rounded-full text-[9px] font-black text-slate-950"
							style={optionStyle(option)}
						>
							{index + 1}
						</span>
						<span class="block min-w-0 truncate">{option.label}</span>
					</span>
					<X size={12} class="justify-self-end opacity-60" />
				</button>
			{/each}
		</div>
	</div>

	<form
		class="flex min-w-0 items-center gap-2 bg-[#08131d] px-2"
		onsubmit={(event) => {
			event.preventDefault();
			addOption();
		}}
	>
		<input
			bind:value={inputValue}
			aria-label="룰렛 선택지 추가"
			placeholder="선택지 추가"
			class="min-w-0 flex-1 bg-transparent py-2.5 text-sm font-medium text-slate-50 outline-none placeholder:text-slate-500"
			disabled={spinning || options.length >= 12}
		/>
		<button
			type="submit"
			aria-label="선택지 추가"
			class="grid size-8 place-items-center rounded-sm text-cyan-100 hover:bg-cyan-400/10 disabled:cursor-not-allowed disabled:opacity-50"
			disabled={!inputValue.trim() || spinning || options.length >= 12}
		>
			<Plus size={16} />
		</button>
	</form>

	<button
		type="button"
		class="inline-flex h-10 items-center justify-center gap-2 rounded-sm bg-cyan-400/12 px-4 text-sm font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-60"
		disabled={spinning || !canSpin}
		onclick={spinRoulette}
	>
		<Disc3 size={16} />
		{spinning ? '돌리는 중' : result ? '다시 돌리기' : canSpin ? '돌리기' : '선택지 필요'}
	</button>
</div>

<style>
	.roulette-wheel {
		transition: transform 900ms cubic-bezier(0.18, 0.85, 0.18, 1);
	}

	.roulette-chip-number {
		background-color: var(--roulette-tone);
	}

	@media (prefers-reduced-motion: reduce) {
		.roulette-wheel {
			transition-duration: 1ms;
		}
	}
</style>
