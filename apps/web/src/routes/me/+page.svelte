<script lang="ts">
	import { Gamepad2, Server, ShieldCheck, Sparkles } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import RiotAccountManager from '$lib/components/workflow/RiotAccountManager.svelte';
	import UserChampionPreferencesEditor from '$lib/components/workflow/UserChampionPreferencesEditor.svelte';
	import UserDefaultGuildSettingsPanel from '$lib/components/workflow/UserDefaultGuildSettingsPanel.svelte';
	import UserPrivacySettingsPanel from '$lib/components/workflow/UserPrivacySettingsPanel.svelte';
	import type { PageProps } from './$types';

	type MeTab = 'champions' | 'riot' | 'guild' | 'privacy';

	let { data }: PageProps = $props();
	let activeTab = $state<MeTab>(untrack(() => data.initialTab));

	const tabs = [
		{ value: 'champions' as const, label: '선호 챔피언', icon: Sparkles },
		{ value: 'riot' as const, label: 'Riot 계정', icon: Gamepad2 },
		{ value: 'guild' as const, label: '대표 길드', icon: Server },
		{ value: 'privacy' as const, label: '개인정보', icon: ShieldCheck }
	];
</script>

<svelte:head>
	<title>내 정보 | MonkeyBot</title>
</svelte:head>

<div class="mx-auto max-w-6xl space-y-4">
	<WorkspaceHeader kicker="계정" title="내 정보" description="탐솔 · Discord 계정">
		{#snippet actions()}
			<div
				class="flex max-w-full overflow-x-auto border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
				role="tablist"
				aria-label="내 정보 설정"
			>
				{#each tabs as tab (tab.value)}
					<button
						type="button"
						role="tab"
						aria-selected={activeTab === tab.value}
						onclick={() => (activeTab = tab.value)}
						class={`inline-flex h-8 shrink-0 items-center gap-1.5 px-3 text-xs font-semibold ${
							activeTab === tab.value
								? 'rounded-sm bg-cyan-400/12 text-cyan-100'
								: 'text-slate-500 hover:text-slate-200'
						}`}
					>
						<tab.icon size={14} aria-hidden="true" />
						{tab.label}
					</button>
				{/each}
			</div>
		{/snippet}
	</WorkspaceHeader>

	{#if activeTab === 'champions'}
		<UserChampionPreferencesEditor catalog={data.championCatalog} />
	{:else if activeTab === 'riot'}
		<RiotAccountManager version={data.championCatalog.version} />
	{:else if activeTab === 'guild'}
		<UserDefaultGuildSettingsPanel />
	{:else}
		<UserPrivacySettingsPanel />
	{/if}
</div>
