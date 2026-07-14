<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { CalendarRange, FileClock, Settings, UsersRound } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { LayoutProps } from './$types';

	type AdminSection = 'members' | 'settings' | 'logs' | 'seasons';

	let { data, children }: LayoutProps = $props();

	const guild = $derived(data.guild);
	const adminPath = $derived(resolve('/g/[guildId]/admin', { guildId: data.guildId }));
	const seasonsPath = $derived(resolve('/g/[guildId]/admin/seasons', { guildId: data.guildId }));
	const activeSection = $derived(
		getActiveSection(page.url.pathname, page.url.searchParams.get('tab'))
	);
	const usesPanelScrollArea = $derived(activeSection === 'members' || activeSection === 'logs');
	const tabs = $derived([
		{
			value: 'members' as const,
			label: '멤버',
			shortLabel: '멤버',
			icon: UsersRound,
			href: `${adminPath}?tab=members`
		},
		{
			value: 'settings' as const,
			label: '길드 설정',
			shortLabel: '설정',
			icon: Settings,
			href: `${adminPath}?tab=settings`
		},
		{
			value: 'seasons' as const,
			label: '시즌',
			shortLabel: '시즌',
			icon: CalendarRange,
			href: seasonsPath
		},
		{
			value: 'logs' as const,
			label: '감사 로그',
			shortLabel: '로그',
			icon: FileClock,
			href: `${adminPath}?tab=logs`
		}
	]);

	function getActiveSection(pathname: string, tab: string | null): AdminSection {
		if (pathname === seasonsPath || pathname.startsWith(`${seasonsPath}/`)) return 'seasons';
		if (tab === 'settings' || tab === 'logs') return tab;
		return 'members';
	}
</script>

<svelte:head>
	<title>{guild.displayName} 관리 | MonkeyBot</title>
</svelte:head>

<div class="flex min-h-0 flex-1 flex-col gap-4">
	<div class="shrink-0">
		<WorkspaceHeader
			kicker="길드 관리"
			title={guild.displayName}
			description={`${guild.memberCount}명 · ${guild.currentUserRole === 'OPERATOR' ? '운영자' : '참가자'}`}
		>
			{#snippet actions()}
				<nav
					class="grid w-full max-w-full grid-cols-4 border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1 sm:flex sm:w-auto"
					aria-label="길드 관리 메뉴"
				>
					{#each tabs as tab (tab.value)}
						<a
							href={tab.href}
							aria-current={activeSection === tab.value ? 'page' : undefined}
							class={`inline-flex h-8 min-w-0 items-center justify-center gap-1 px-1 text-xs font-semibold sm:shrink-0 sm:gap-1.5 sm:px-3 ${
								activeSection === tab.value
									? 'rounded-sm bg-cyan-400/12 text-cyan-100'
									: 'text-slate-500 hover:text-slate-200'
							}`}
						>
							<tab.icon size={14} aria-hidden="true" />
							<span class="sm:hidden">{tab.shortLabel}</span>
							<span class="hidden sm:inline">{tab.label}</span>
						</a>
					{/each}
				</nav>
			{/snippet}
		</WorkspaceHeader>
	</div>

	<div
		class={`min-h-0 flex-1 ${
			usesPanelScrollArea
				? 'flex flex-col overflow-hidden'
				: 'overflow-y-auto overscroll-contain pr-px'
		}`}
	>
		{@render children()}
	</div>
</div>
