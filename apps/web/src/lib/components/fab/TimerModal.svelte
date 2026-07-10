<script lang="ts">
	import { onDestroy } from 'svelte';
	import { Dialog } from 'bits-ui';
	import { Minus, Pause, Play, Plus, RotateCcw, Timer, X } from 'lucide-svelte';

	interface Props {
		open: boolean;
	}

	const defaultDurationSeconds = 300;

	const presets = [
		{ label: '1분', seconds: 60 },
		{ label: '3분', seconds: 180 },
		{ label: '5분', seconds: 300 },
		{ label: '10분', seconds: 600 }
	];

	let { open = $bindable() }: Props = $props();

	let running = $state(false);
	let selectedSeconds = $state(defaultDurationSeconds);
	let remainingMs = $state(defaultDurationSeconds * 1000);
	let endsAt = 0;
	let ticker: ReturnType<typeof globalThis.setInterval> | null = null;
	let completed = $state(false);

	function pad(value: number) {
		return String(value).padStart(2, '0');
	}

	function formatTime(valueMs: number) {
		const totalSeconds = Math.ceil(Math.max(0, valueMs) / 1000);
		const minutes = Math.floor(totalSeconds / 60);
		const seconds = totalSeconds % 60;

		return `${pad(minutes)}:${pad(seconds)}`;
	}

	function clearTicker() {
		if (!ticker) return;
		globalThis.clearInterval(ticker);
		ticker = null;
	}

	function syncRemaining() {
		if (!running) return;

		remainingMs = Math.max(0, endsAt - globalThis.performance.now());

		if (remainingMs <= 0) {
			running = false;
			completed = true;
			clearTicker();
		}
	}

	function setDuration(seconds: number) {
		if (running) return;

		const nextSeconds = Math.min(99 * 60 + 59, Math.max(1, seconds));
		selectedSeconds = nextSeconds;
		remainingMs = nextSeconds * 1000;
		completed = false;
	}

	function adjustDuration(deltaSeconds: number) {
		setDuration(selectedSeconds + deltaSeconds);
	}

	function start() {
		if (running || remainingMs <= 0) return;

		completed = false;
		endsAt = globalThis.performance.now() + remainingMs;
		running = true;
		ticker = globalThis.setInterval(syncRemaining, 200);
		syncRemaining();
	}

	function pause() {
		if (!running) return;

		syncRemaining();
		running = false;
		clearTicker();
	}

	function reset() {
		running = false;
		remainingMs = selectedSeconds * 1000;
		completed = false;
		clearTicker();
	}

	onDestroy(clearTicker);
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex h-12 items-center justify-between px-4">
				<div class="flex items-center gap-2">
					<Timer size={16} class="text-emerald-100" />
					<Dialog.Title class="text-sm font-semibold text-slate-50">타이머</Dialog.Title>
				</div>
				<Dialog.Close
					class="grid size-8 place-items-center rounded-sm text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
					aria-label="타이머 닫기"
				>
					<X size={16} />
				</Dialog.Close>
			</div>

			<div class="grid gap-4 p-4">
				<section class="grid gap-3 bg-[var(--color-app-bg-soft)] p-4">
					<div class="flex items-center justify-between gap-3">
						<button
							type="button"
							class="grid size-10 place-items-center rounded-sm text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-40"
							aria-label="타이머 30초 감소"
							disabled={running || selectedSeconds <= 30}
							onclick={() => adjustDuration(-30)}
						>
							<Minus size={18} />
						</button>
						<p
							class={`font-mono text-6xl font-semibold tabular-nums sm:text-7xl ${
								completed ? 'text-emerald-100' : 'text-slate-50'
							}`}
						>
							{formatTime(remainingMs)}
						</p>
						<button
							type="button"
							class="grid size-10 place-items-center rounded-sm text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-40"
							aria-label="타이머 30초 증가"
							disabled={running}
							onclick={() => adjustDuration(30)}
						>
							<Plus size={18} />
						</button>
					</div>

					<div class="grid grid-cols-4 gap-1">
						{#each presets as preset (preset.seconds)}
							<button
								type="button"
								class={`h-9 rounded-sm text-xs font-semibold ${
									selectedSeconds === preset.seconds
										? 'bg-emerald-300/10 text-emerald-100'
										: 'text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100'
								} disabled:cursor-not-allowed disabled:opacity-40`}
								disabled={running}
								onclick={() => setDuration(preset.seconds)}
							>
								{preset.label}
							</button>
						{/each}
					</div>

					<div class="grid grid-cols-2 gap-2">
						<button
							type="button"
							class="grid h-11 place-items-center rounded-sm bg-emerald-300/10 text-emerald-100 hover:bg-emerald-300/15 disabled:cursor-not-allowed disabled:opacity-40"
							aria-label={running ? '타이머 일시정지' : '타이머 시작'}
							disabled={!running && remainingMs <= 0}
							onclick={running ? pause : start}
						>
							{#if running}
								<Pause size={18} />
							{:else}
								<Play size={18} />
							{/if}
						</button>
						<button
							type="button"
							class="grid h-11 place-items-center rounded-sm text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-40"
							aria-label="타이머 리셋"
							disabled={remainingMs === selectedSeconds * 1000 && !running && !completed}
							onclick={reset}
						>
							<RotateCcw size={18} />
						</button>
					</div>
				</section>

				<div
					class={`px-3 py-2 text-center text-sm font-semibold ${
						completed
							? 'bg-emerald-300/10 text-emerald-100'
							: running
								? 'bg-cyan-400/10 text-cyan-100'
								: 'bg-[var(--color-app-bg-soft)] text-slate-500'
					}`}
				>
					{#if completed}
						타이머 종료
					{:else if running}
						진행 중
					{:else}
						대기 중
					{/if}
				</div>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
