<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import UserChampionPreferencesEditor from '$lib/components/workflow/UserChampionPreferencesEditor.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>{data.displayName} 프로필 | MonkeyBot</title>
</svelte:head>

<div class="mx-auto max-w-6xl space-y-5">
	<WorkspaceHeader
		kicker="사용자"
		title={data.displayName}
		description="MSM Seoul · 내전 참가자"
		metrics={[
			{ label: 'Discord', value: data.discordId, tone: 'neutral' },
			{ label: '시즌', value: '3', tone: 'info' }
		]}
	/>
	<UserChampionPreferencesEditor catalog={data.championCatalog} />
</div>
