<script lang="ts">
	import { Dialog, Tabs } from 'bits-ui';
	import { X } from 'lucide-svelte';
	import CoinTossGame from './CoinTossGame.svelte';
	import LadderGame from './LadderGame.svelte';
	import RouletteGame from './RouletteGame.svelte';

	type MiniGameTab = 'coin' | 'ladder' | 'roulette';

	interface Props {
		open: boolean;
	}

	let { open = $bindable() }: Props = $props();
	let selectedTab = $state<MiniGameTab>('coin');
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-3xl -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<div class="flex h-12 items-center justify-between px-4">
				<Dialog.Title class="text-sm font-semibold text-slate-50">미니게임</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center rounded-sm text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
					aria-label="미니게임 닫기"
				>
					<X size={16} />
				</Dialog.Close>
			</div>

			<Tabs.Root
				value={selectedTab}
				onValueChange={(value) => {
					if (value === 'ladder' || value === 'roulette') selectedTab = value;
					else selectedTab = 'coin';
				}}
			>
				<Tabs.List
					aria-label="미니게임 종류"
					class="grid grid-cols-3 gap-1 bg-[var(--color-app-bg-soft)] p-1"
				>
					<Tabs.Trigger
						value="coin"
						class="h-9 rounded-sm text-xs font-semibold text-slate-400 outline-none hover:text-slate-100 data-[state=active]:rounded-md data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100"
					>
						코인토스
					</Tabs.Trigger>
					<Tabs.Trigger
						value="ladder"
						class="h-9 rounded-sm text-xs font-semibold text-slate-400 outline-none hover:text-slate-100 data-[state=active]:rounded-md data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100"
					>
						사다리
					</Tabs.Trigger>
					<Tabs.Trigger
						value="roulette"
						class="h-9 rounded-sm text-xs font-semibold text-slate-400 outline-none hover:text-slate-100 data-[state=active]:rounded-md data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100"
					>
						룰렛
					</Tabs.Trigger>
				</Tabs.List>

				<Tabs.Content value="coin">
					<CoinTossGame />
				</Tabs.Content>
				<Tabs.Content value="ladder">
					<LadderGame />
				</Tabs.Content>
				<Tabs.Content value="roulette">
					<RouletteGame />
				</Tabs.Content>
			</Tabs.Root>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
