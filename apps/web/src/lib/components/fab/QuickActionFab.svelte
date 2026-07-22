<script lang="ts">
	import { cubicOut } from 'svelte/easing';
	import { fly, scale } from 'svelte/transition';
	import { Plus, X } from 'lucide-svelte';
	import QuickActionMenuItem from './QuickActionMenuItem.svelte';

	interface Props {
		label?: string;
	}

	let { label = '빠른 실행' }: Props = $props();

	let open = $state(false);
	let miniGameOpen = $state(false);
	let timerOpen = $state(false);
	let loadingTool = $state<'game' | 'timer' | null>(null);
	let toolLoadError = $state<string | null>(null);
	let MiniGameModalComponent = $state<typeof import('./MiniGameModal.svelte').default | null>(null);
	let TimerModalComponent = $state<typeof import('./TimerModal.svelte').default | null>(null);

	function toggleOpen() {
		open = !open;
	}

	function closeFab() {
		open = false;
	}

	async function openMiniGame() {
		if (loadingTool) return;

		loadingTool = 'game';
		toolLoadError = null;
		try {
			MiniGameModalComponent ??= (await import('./MiniGameModal.svelte')).default;
			closeFab();
			miniGameOpen = true;
		} catch {
			toolLoadError =
				'미니게임 화면을 불러오지 못했습니다. 네트워크 연결을 확인한 뒤 다시 시도해 주세요.';
		} finally {
			loadingTool = null;
		}
	}

	async function openTimer() {
		if (loadingTool) return;

		loadingTool = 'timer';
		toolLoadError = null;
		try {
			TimerModalComponent ??= (await import('./TimerModal.svelte')).default;
			closeFab();
			timerOpen = true;
		} catch {
			toolLoadError =
				'타이머 화면을 불러오지 못했습니다. 네트워크 연결을 확인한 뒤 다시 시도해 주세요.';
		} finally {
			loadingTool = null;
		}
	}
</script>

<div class="quick-action-fab fixed right-6 z-40 sm:right-10">
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

				<div class="grid gap-1 p-2" aria-busy={loadingTool !== null}>
					<QuickActionMenuItem label="미니게임" tone="game" onSelect={openMiniGame} />
					<QuickActionMenuItem label="타이머" tone="timer" onSelect={openTimer} />
					{#if loadingTool}
						<p class="px-2 py-1 text-xs text-slate-400" role="status">
							{loadingTool === 'game' ? '미니게임' : '타이머'} 화면을 불러오는 중입니다.
						</p>
					{:else if toolLoadError}
						<p class="px-2 py-1 text-xs leading-5 text-rose-200" role="alert">
							{toolLoadError}
						</p>
					{/if}
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

{#if MiniGameModalComponent}
	<MiniGameModalComponent bind:open={miniGameOpen} />
{/if}

{#if TimerModalComponent}
	<TimerModalComponent bind:open={timerOpen} />
{/if}

<style>
	.quick-action-fab {
		bottom: calc(5rem + env(safe-area-inset-bottom));
	}

	@media (min-width: 640px) {
		.quick-action-fab {
			bottom: calc(5.5rem + env(safe-area-inset-bottom));
		}
	}
</style>
