<script lang="ts">
	import { Gamepad2, Timer } from 'lucide-svelte';

	interface Props {
		label: string;
		tone: 'game' | 'timer';
		onSelect: () => void;
	}

	let { label, tone, onSelect }: Props = $props();

	const iconToneClass = {
		game: 'bg-cyan-400/10 text-cyan-100',
		timer: 'bg-emerald-300/10 text-emerald-100'
	};
</script>

<button
	type="button"
	class="grid grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-2 rounded-sm px-2 py-2 text-left text-slate-200 hover:bg-[var(--color-app-hover)]"
	onclick={onSelect}
>
	<span class={`grid size-9 place-items-center rounded-sm ${iconToneClass[tone]}`}>
		{#if tone === 'game'}
			<Gamepad2 size={17} />
		{:else}
			<Timer size={17} />
		{/if}
	</span>
	<span class="min-w-0">
		<span class="block text-sm font-semibold">{label}</span>
	</span>
</button>
