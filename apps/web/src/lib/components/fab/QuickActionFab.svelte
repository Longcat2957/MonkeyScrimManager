<script lang="ts">
	import { cubicOut } from 'svelte/easing';
	import { fly, scale } from 'svelte/transition';
	import { Plus, X } from 'lucide-svelte';
	import QuickActionMenuItem from './QuickActionMenuItem.svelte';
	import MiniGameModal from './MiniGameModal.svelte';
	import QuickRecordSearchModal from './QuickRecordSearchModal.svelte';
	import TimerModal from './TimerModal.svelte';

	interface Props {
		label?: string;
	}

	let { label = '빠른 실행' }: Props = $props();

	let open = $state(false);
	let miniGameOpen = $state(false);
	let quickSearchOpen = $state(false);
	let timerOpen = $state(false);

	function toggleOpen() {
		open = !open;
	}

	function closeFab() {
		open = false;
	}

	function openMiniGame() {
		closeFab();
		miniGameOpen = true;
	}

	function openQuickSearch() {
		closeFab();
		quickSearchOpen = true;
	}

	function openTimer() {
		closeFab();
		timerOpen = true;
	}
</script>

<div class="fixed right-6 bottom-6 z-40 sm:right-10 sm:bottom-10">
	{#if open}
		<div
			class="mb-3 w-[calc(100vw-2rem)] max-w-[22rem]"
			transition:fly={{ y: 10, duration: 140, easing: cubicOut }}
		>
			<div
				class="border border-cyan-400/20 bg-[var(--color-app-surface)] shadow-2xl shadow-black/45"
			>
				<div class="flex h-11 items-center justify-between px-3">
					<p class="text-sm font-semibold text-slate-100">{label}</p>
					<button
						type="button"
						aria-label="빠른 실행 닫기"
						class="grid size-8 place-items-center rounded-sm text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
						onclick={closeFab}
					>
						<X size={16} />
					</button>
				</div>

				<div class="grid gap-1 p-2">
					<QuickActionMenuItem label="미니게임" tone="game" onSelect={openMiniGame} />
					<QuickActionMenuItem label="전적검색" tone="record" onSelect={openQuickSearch} />
					<QuickActionMenuItem label="타이머" tone="timer" onSelect={openTimer} />
				</div>
			</div>
		</div>
	{/if}

	<button
		type="button"
		aria-label={open ? '빠른 실행 닫기' : '빠른 실행 열기'}
		aria-expanded={open}
		class={`ml-auto grid size-15 place-items-center rounded-full border shadow-2xl shadow-black/45 transition ${
			open
				? 'border-cyan-300/45 bg-cyan-400/16 text-cyan-50'
				: 'border-cyan-400/25 bg-cyan-400/10 text-cyan-100 hover:bg-cyan-400/15'
		}`}
		onclick={toggleOpen}
		transition:scale={{ duration: 120, easing: cubicOut }}
	>
		{#if open}
			<X size={26} />
		{:else}
			<Plus size={30} strokeWidth={2.5} />
		{/if}
	</button>
</div>

<MiniGameModal bind:open={miniGameOpen} />
<QuickRecordSearchModal bind:open={quickSearchOpen} />
<TimerModal bind:open={timerOpen} />
