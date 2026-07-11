<script lang="ts">
	import { resolve } from '$app/paths';
	import { History } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import OperationsQueueBoard from '$lib/components/workflow/OperationsQueueBoard.svelte';
	import { getMockGuild } from '$lib/fixtures/guilds';
	import { mockOperationsQueue } from '$lib/fixtures/operations';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const guild = $derived(getMockGuild(data.guildId));
	const guildBase = $derived(resolve('/g/[guildId]', { guildId: data.guildId }));
	const operations = $derived(
		mockOperationsQueue.map((item) => ({ ...item, href: `${guildBase}${item.href}` }))
	);
</script>

<svelte:head>
	<title>{guild.name} | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="길드 운영"
		title={guild.name}
		description={`${guild.memberCount}명 · ${guild.role}`}
	>
		{#snippet actions()}
			<a
				href={resolve('/g/[guildId]/records', { guildId: data.guildId })}
				class="inline-flex h-10 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 hover:text-slate-50"
			>
				<History size={15} aria-hidden="true" /> 내전 전적
			</a>
		{/snippet}
	</WorkspaceHeader>

	<OperationsQueueBoard items={operations} />
</div>
