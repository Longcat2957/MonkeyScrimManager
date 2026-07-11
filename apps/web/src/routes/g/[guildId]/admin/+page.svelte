<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { FileClock, History, Settings, UsersRound } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import GuildAuditLogPanel from '$lib/components/workflow/GuildAuditLogPanel.svelte';
	import GuildMemberManagementPanel from '$lib/components/workflow/GuildMemberManagementPanel.svelte';
	import GuildOperationsAdminPanel from '$lib/components/workflow/GuildOperationsAdminPanel.svelte';
	import GuildSettingsAdminPanel from '$lib/components/workflow/GuildSettingsAdminPanel.svelte';
	import { getMockGuild } from '$lib/fixtures/guilds';
	import type { PageProps } from './$types';

	type AdminTab = 'members' | 'operations' | 'settings' | 'logs';

	let { data }: PageProps = $props();

	const guild = $derived(getMockGuild(data.guildId));
	const adminPath = $derived(resolve('/g/[guildId]/admin', { guildId: data.guildId }));
	const activeTab = $derived(parseTab(page.url.searchParams.get('tab')));
	const tabs = [
		{ value: 'members' as const, label: '멤버', icon: UsersRound },
		{ value: 'operations' as const, label: '운영', icon: History },
		{ value: 'settings' as const, label: '길드 설정', icon: Settings },
		{ value: 'logs' as const, label: '감사 로그', icon: FileClock }
	];

	function parseTab(value: string | null): AdminTab {
		return value === 'operations' || value === 'settings' || value === 'logs' ? value : 'members';
	}
</script>

<svelte:head>
	<title>{guild.name} 관리 | MonkeyBot</title>
</svelte:head>

<div class="space-y-4">
	<WorkspaceHeader
		kicker="길드 관리"
		title={guild.name}
		description={`${guild.memberCount}명 · ${guild.role}`}
	>
		{#snippet actions()}
			<nav
				class="grid w-full max-w-full grid-cols-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1 sm:flex sm:w-auto"
				aria-label="길드 관리 메뉴"
			>
				{#each tabs as tab (tab.value)}
					<a
						href={`${adminPath}?tab=${tab.value}`}
						aria-current={activeTab === tab.value ? 'page' : undefined}
						class={`inline-flex h-8 min-w-0 items-center justify-center gap-1.5 px-3 text-xs font-semibold sm:shrink-0 ${
							activeTab === tab.value
								? 'rounded-sm bg-cyan-400/12 text-cyan-100'
								: 'text-slate-500 hover:text-slate-200'
						}`}
					>
						<tab.icon size={14} aria-hidden="true" />
						{tab.label}
					</a>
				{/each}
			</nav>
		{/snippet}
	</WorkspaceHeader>

	{#if activeTab === 'members'}
		<GuildMemberManagementPanel />
	{:else if activeTab === 'operations'}
		<GuildOperationsAdminPanel />
	{:else if activeTab === 'settings'}
		<GuildSettingsAdminPanel />
	{:else}
		<GuildAuditLogPanel />
	{/if}
</div>
