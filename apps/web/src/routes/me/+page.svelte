<script lang="ts">
	import { goto, invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Gamepad2, Server, ShieldCheck, Sparkles } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import RiotAccountManager from '$lib/components/workflow/RiotAccountManager.svelte';
	import UserChampionPreferencesEditor from '$lib/components/workflow/UserChampionPreferencesEditor.svelte';
	import UserDefaultGuildSettingsPanel from '$lib/components/workflow/UserDefaultGuildSettingsPanel.svelte';
	import UserPrivacySettingsPanel from '$lib/components/workflow/UserPrivacySettingsPanel.svelte';
	import type { PageProps } from './$types';

	type MeTab = 'champions' | 'riot' | 'guild' | 'privacy';

	let { data }: PageProps = $props();
	const activeTab = $derived<MeTab>(data.initialTab);

	const tabs = [
		{ value: 'champions' as const, label: '선호 챔피언', icon: Sparkles },
		{ value: 'riot' as const, label: 'Riot 계정', icon: Gamepad2 },
		{ value: 'guild' as const, label: '대표 길드', icon: Server },
		{ value: 'privacy' as const, label: '개인정보', icon: ShieldCheck }
	];

	function selectTab(tab: MeTab) {
		void goto(`${resolve('/me')}?tab=${tab}`, { replaceState: true, noScroll: true });
	}

	function readCookie(name: string): string | null {
		const prefix = `${name}=`;
		const value = globalThis.document.cookie
			.split(';')
			.map((cookie) => cookie.trim())
			.find((cookie) => cookie.startsWith(prefix));
		return value ? decodeURIComponent(value.slice(prefix.length)) : null;
	}

	async function saveDefaultGuild(guildId: string) {
		const csrfToken = readCookie('monkeybot_csrf');
		if (!csrfToken) throw new Error('csrf token is unavailable');
		const response = await globalThis.fetch(
			resolve('/api/[...path]', { path: 'me/default-guild' }),
			{
				method: 'PUT',
				credentials: 'same-origin',
				headers: {
					accept: 'application/json',
					'content-type': 'application/json',
					'X-CSRF-Token': csrfToken
				},
				body: JSON.stringify({ guildId })
			}
		);
		if (!response.ok) throw new Error('default guild update failed');
		await invalidateAll();
	}
</script>

<svelte:head>
	<title>내 정보 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="계정"
		title="내 정보"
		description={`${data.user.displayName} · Discord 계정`}
	>
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
						onclick={() => selectTab(tab.value)}
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

	{#if activeTab === 'champions' && data.championCatalog}
		<UserChampionPreferencesEditor
			catalog={data.championCatalog}
			initialPreferences={data.championPreferences}
		/>
	{:else if activeTab === 'riot'}
		<RiotAccountManager initialAccounts={data.riotAccounts} />
	{:else if activeTab === 'guild'}
		<UserDefaultGuildSettingsPanel
			initialGuildId={data.defaultGuildId}
			guilds={data.guilds}
			onSave={saveDefaultGuild}
		/>
	{:else}
		<UserPrivacySettingsPanel initialSettings={data.privacySettings} />
	{/if}
</div>
