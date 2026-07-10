<script lang="ts">
	import { laneLabel, type Lane } from '$lib/fixtures/workflow';

	interface Props {
		role: Lane;
		size?: 'sm' | 'md';
	}

	let { role, size = 'md' }: Props = $props();

	const sizeClass = {
		sm: 'size-5',
		md: 'size-7'
	};

	const iconPath: Record<Lane, string> = {
		TOP: '/riot/positions/top.svg',
		JUNGLE: '/riot/positions/jungle.svg',
		MID: '/riot/positions/mid.svg',
		BOTTOM: '/riot/positions/bottom.svg',
		SUPPORT: '/riot/positions/support.svg'
	};
</script>

<span
	class={`inline-grid ${sizeClass[size]} place-items-center`}
	aria-label={`${laneLabel[role]} 포지션`}
	title={`${laneLabel[role]} 포지션`}
>
	<img class="size-full object-contain" src={iconPath[role]} alt="" loading="lazy" />
</span>
