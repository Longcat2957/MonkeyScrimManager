<script lang="ts">
	import { Coins } from 'lucide-svelte';

	type CoinSide = 'front' | 'back';

	let result = $state<CoinSide | null>(null);
	let tossCount = $state(0);
	let flipping = $state(false);

	function randomSide(): CoinSide {
		const buffer = new Uint32Array(1);
		globalThis.crypto.getRandomValues(buffer);
		return buffer[0] % 2 === 0 ? 'front' : 'back';
	}

	function tossCoin() {
		if (flipping) return;

		flipping = true;
		result = randomSide();
		tossCount += 1;

		globalThis.setTimeout(() => {
			flipping = false;
		}, 420);
	}

	function resultLabel(side: CoinSide | null): string {
		if (side === 'front') return '앞면';
		if (side === 'back') return '뒷면';
		return '대기';
	}

	function resultMark(side: CoinSide | null): string {
		if (side === 'front') return 'H';
		if (side === 'back') return 'T';
		return '?';
	}
</script>

<div class="grid gap-4 p-4">
	<div class="grid gap-3 bg-[var(--color-app-bg-soft)] p-4">
		<div class="flex items-center justify-between gap-3">
			<div class="flex items-center gap-2">
				<Coins size={17} class="text-cyan-100" />
				<p class="text-sm font-semibold text-slate-100">코인토스</p>
			</div>
			<p class="text-xs font-semibold text-slate-500">{tossCount}회</p>
		</div>

		<div class="grid place-items-center py-4">
			<div
				class={`grid size-28 place-items-center rounded-full border text-center shadow-xl shadow-black/30 transition-transform duration-500 ${
					flipping ? 'coin-flip' : ''
				} ${
					result === 'front'
						? 'border-cyan-300/45 bg-cyan-400/14 text-cyan-50 shadow-cyan-950/30'
						: result === 'back'
							? 'border-amber-300/45 bg-amber-300/12 text-amber-50 shadow-amber-950/30'
							: 'border-slate-700 bg-slate-900/70 text-slate-400'
				}`}
				aria-live="polite"
			>
				<div>
					<p class="text-4xl font-black leading-none tracking-normal">{resultMark(result)}</p>
					<p class="mt-2 text-sm font-semibold">{resultLabel(result)}</p>
				</div>
			</div>
		</div>
	</div>

	<button
		type="button"
		class="inline-flex h-10 items-center justify-center gap-2 rounded-sm bg-cyan-400/12 px-4 text-sm font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-60"
		disabled={flipping}
		onclick={tossCoin}
	>
		<Coins size={16} />
		{flipping ? '던지는 중' : result ? '다시 던지기' : '던지기'}
	</button>
</div>

<style>
	.coin-flip {
		transform: rotateY(720deg) scale(1.04);
	}

	@media (prefers-reduced-motion: reduce) {
		.coin-flip {
			transform: none;
		}
	}
</style>
