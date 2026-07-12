<script lang="ts">
	import { resolve } from '$app/paths';
	import { History } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import OperationsQueueBoard from '$lib/components/workflow/OperationsQueueBoard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const guild = $derived(data.guild);
	const operations = $derived(data.operations);
</script>

<svelte:head>
	<title>{guild.displayName} | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="길드 운영"
		title={guild.displayName}
		description={`${guild.memberCount}명 · ${guild.currentUserRole === 'OPERATOR' ? '운영자' : '참가자'}`}
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
