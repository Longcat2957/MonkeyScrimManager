<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head>
	<title>{data.profile.displayName} 프로필 | MonkeyBot</title>
</svelte:head>

<div class="mx-auto max-w-6xl space-y-5">
	<WorkspaceHeader
		kicker="사용자 프로필"
		title={data.profile.displayName}
		description="공개 사용자 정보"
	/>

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<dl class="grid sm:grid-cols-3">
			<div
				class="grid gap-1 border-b border-[var(--color-app-border)] px-4 py-4 sm:border-r sm:border-b-0"
			>
				<dt class="text-[11px] font-semibold text-slate-500">Discord ID</dt>
				<dd class="break-all text-sm font-semibold text-slate-100">{data.profile.discordId}</dd>
			</div>
			<div
				class="grid gap-1 border-b border-[var(--color-app-border)] px-4 py-4 sm:border-r sm:border-b-0"
			>
				<dt class="text-[11px] font-semibold text-slate-500">활동 길드</dt>
				<dd class="text-xl font-semibold text-slate-100">
					{data.profile.activeGuildCount}<span class="ml-1 text-xs text-slate-500">개</span>
				</dd>
			</div>
			<div class="grid gap-1 px-4 py-4">
				<dt class="text-[11px] font-semibold text-slate-500">시즌 수</dt>
				<dd class="text-xl font-semibold text-slate-100">
					{data.profile.seasonCount}<span class="ml-1 text-xs text-slate-500">개</span>
				</dd>
			</div>
		</dl>
	</section>
</div>
