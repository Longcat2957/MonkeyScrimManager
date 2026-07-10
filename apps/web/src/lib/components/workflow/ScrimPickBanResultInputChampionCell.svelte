<script lang="ts">
	import { championIconUrl } from '$lib/datadragon';
	import type { ScrimPickBanResultInputChampion } from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import {
		findScrimPickBanResultInputChampionById,
		matchScrimPickBanResultInputChampion,
		suggestScrimPickBanResultInputChampions
	} from './scrimPickBanResultInputHelpers';

	interface Props {
		label?: string;
		value: string | null;
		champions: ScrimPickBanResultInputChampion[];
		version: string;
		active: boolean;
		compact?: boolean;
		embedded?: boolean;
		onFocus: () => void;
		onCommit: (championId: string | null) => void;
	}

	let {
		label,
		value,
		champions,
		version,
		active,
		compact = false,
		embedded = false,
		onFocus,
		onCommit
	}: Props = $props();

	const selected = $derived(findScrimPickBanResultInputChampionById(value, champions));
	const listboxId = $props.id();
	let text = $state('');
	let syncedValue = $state<string | null>(null);
	let focused = $state(false);
	let highlightedIndex = $state(0);
	const suggestions = $derived(suggestScrimPickBanResultInputChampions(text, champions));
	const showSuggestions = $derived(focused && suggestions.length > 0);

	$effect(() => {
		if (value === syncedValue) return;
		syncedValue = value;
		text = selected?.name ?? '';
		highlightedIndex = 0;
	});

	function commit() {
		const raw = text.trim();
		if (!raw) {
			onCommit(null);
			return;
		}

		const matched = matchScrimPickBanResultInputChampion(raw, champions);
		if (!matched) {
			text = selected?.name ?? '';
			return;
		}

		onCommit(matched.id);
		syncedValue = matched.id;
	}

	function commitChampion(champion: ScrimPickBanResultInputChampion) {
		text = champion.name;
		focused = false;
		highlightedIndex = 0;
		syncedValue = champion.id;
		onCommit(champion.id);
	}

	function surfaceClass(): string {
		if (embedded) {
			if (active)
				return 'rounded-sm bg-slate-900/70 shadow-[inset_0_0_0_1px_rgba(103,232,249,0.45)]';
			if (selected) return 'rounded-sm bg-slate-800/65';
			return 'rounded-sm bg-slate-950/35 hover:bg-slate-900/55';
		}

		if (active) return 'rounded-md border-cyan-300/70 bg-cyan-400/12 ring-1 ring-cyan-300/35';
		if (selected) return 'rounded-md border-slate-700 bg-slate-800/60';
		return 'rounded-sm border-slate-700 bg-slate-950/25 hover:bg-slate-900/55';
	}
</script>

<div
	class={`relative grid h-full min-w-0 items-center text-left transition ${embedded ? '' : 'border'} ${
		compact
			? 'grid-cols-[2.125rem_minmax(0,1fr)] gap-1 p-1'
			: 'grid-cols-[3rem_minmax(0,1fr)] gap-1.5 p-1.5'
	} ${surfaceClass()}`}
>
	<span
		class={`grid ${compact ? 'size-[34px]' : 'size-12'} place-items-center self-center bg-slate-950/35`}
	>
		{#if selected}
			<img
				src={championIconUrl(selected.imageFull, version)}
				alt={selected.name}
				class="size-full object-cover"
				loading="lazy"
			/>
		{:else}
			<span class="size-full bg-slate-900/80"></span>
		{/if}
	</span>
	<span class="grid min-w-0 content-center">
		{#if label}
			<span class="block truncate text-center text-[10px] font-semibold text-slate-500"
				>{label}</span
			>
		{/if}
		<input
			bind:value={text}
			class={`block w-full min-w-0 bg-transparent font-semibold text-slate-100 outline-none placeholder:text-slate-600 ${label ? 'h-5 text-center text-xs' : 'h-9 text-left text-sm'}`}
			placeholder="챔피언"
			autocomplete="off"
			role="combobox"
			aria-expanded={showSuggestions}
			aria-autocomplete="list"
			aria-controls={listboxId}
			onfocus={() => {
				focused = true;
				onFocus();
			}}
			oninput={() => {
				highlightedIndex = 0;
			}}
			onkeydown={(event) => {
				if (event.key === 'ArrowDown') {
					event.preventDefault();
					highlightedIndex = Math.min(highlightedIndex + 1, suggestions.length - 1);
					return;
				}
				if (event.key === 'ArrowUp') {
					event.preventDefault();
					highlightedIndex = Math.max(highlightedIndex - 1, 0);
					return;
				}
				if (event.key !== 'Enter') return;
				event.preventDefault();
				const suggestedChampion = suggestions[Math.min(highlightedIndex, suggestions.length - 1)];
				if (showSuggestions && suggestedChampion) {
					commitChampion(suggestedChampion);
					return;
				}
				commit();
			}}
			onblur={() => {
				commit();
				focused = false;
			}}
		/>
	</span>
	{#if showSuggestions}
		<div
			id={listboxId}
			class="absolute top-[calc(100%+0.25rem)] right-0 left-0 z-20 grid overflow-hidden rounded-sm bg-[var(--color-app-border-muted)] p-px shadow-xl shadow-black/35"
			role="listbox"
		>
			{#each suggestions as champion, index (champion.id)}
				<button
					type="button"
					class={`grid h-9 grid-cols-[2rem_minmax(0,1fr)] items-center gap-1 px-1.5 text-left ${
						index === highlightedIndex
							? 'bg-cyan-400/12 text-cyan-100'
							: 'bg-slate-950/85 text-slate-200 hover:bg-slate-900'
					}`}
					role="option"
					aria-selected={index === highlightedIndex}
					onmousedown={(event) => {
						event.preventDefault();
						commitChampion(champion);
					}}
				>
					<img
						src={championIconUrl(champion.imageFull, version)}
						alt={champion.name}
						class="size-7 object-cover"
						loading="lazy"
					/>
					<span class="truncate text-xs font-semibold">{champion.name}</span>
				</button>
			{/each}
		</div>
	{/if}
</div>
