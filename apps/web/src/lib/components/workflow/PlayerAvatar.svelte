<script lang="ts">
	import { profileIconUrl } from '$lib/datadragon';
	import type { Lane } from '$lib/fixtures/workflow';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		name: string;
		role: Lane;
		avatarUrl?: string | null;
		profileIconId?: number | null;
		profileIconVersion?: string | null;
		size?: 'xs' | 'sm' | 'md';
	}

	let {
		name,
		role,
		avatarUrl = null,
		profileIconId = null,
		profileIconVersion = null,
		size = 'sm'
	}: Props = $props();

	const sizeClass = {
		xs: 'size-6',
		sm: 'size-7',
		md: 'size-12'
	};

	const iconSize = {
		xs: 'sm',
		sm: 'sm',
		md: 'md'
	} as const;

	const source = $derived(
		avatarUrl ??
			(profileIconId !== null && profileIconVersion
				? profileIconUrl(profileIconId, profileIconVersion)
				: null)
	);
</script>

<span
	class={`grid ${sizeClass[size]} shrink-0 place-items-center overflow-hidden rounded-sm border border-slate-600 bg-slate-950/70`}
	title={name}
>
	{#if source}
		<img class="size-full object-cover" src={source} alt="" loading="lazy" />
	{:else}
		<PositionIcon {role} size={iconSize[size]} />
	{/if}
</span>
