<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, Gavel, History, Swords, Trophy } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import OperationsQueueBoard from '$lib/components/workflow/OperationsQueueBoard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const guild = $derived(data.guild);
	const operations = $derived(data.operations);
	const roleLabel = $derived(guild.currentUserRole === 'OPERATOR' ? '운영자' : '참가자');
	const seasonLabel = $derived(guild.currentSeason?.name ?? '진행 중 시즌 없음');
	const seriesHref = $derived(resolve('/g/[guildId]/series', { guildId: data.guildId }));
	const auctionHref = $derived(resolve('/g/[guildId]/auction', { guildId: data.guildId }));
	const recordsHref = $derived(resolve('/g/[guildId]/records', { guildId: data.guildId }));
	const leaderboardHref = $derived(resolve('/g/[guildId]/leaderboard', { guildId: data.guildId }));
</script>

<svelte:head>
	<title>{guild.displayName} | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="길드 운영"
		title={guild.displayName}
		description={`${seasonLabel} · ${guild.memberCount}명 · ${roleLabel}`}
	>
		{#snippet actions()}
			<a
				href={recordsHref}
				class="inline-flex h-10 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 hover:text-slate-50"
			>
				<History size={15} aria-hidden="true" /> 내전 전적
			</a>
		{/snippet}
	</WorkspaceHeader>

	{#if operations.length > 0}
		<OperationsQueueBoard items={operations} currentUserRole={guild.currentUserRole} />
	{:else}
		<section
			class="grid min-h-64 place-items-center border border-[var(--color-app-border)] bg-[radial-gradient(circle_at_top,rgb(34_211_238_/_0.055),transparent_48%),var(--color-app-surface)] px-5 py-10 text-center"
		>
			<div class="max-w-lg">
				<div
					class="mx-auto grid size-12 place-items-center border border-cyan-300/20 bg-cyan-400/[0.06] text-cyan-200"
				>
					{#if guild.currentUserRole === 'OPERATOR'}
						<Swords size={20} aria-hidden="true" />
					{:else}
						<Trophy size={20} aria-hidden="true" />
					{/if}
				</div>
				<h2 class="mt-4 text-base font-semibold text-slate-100">
					{guild.currentUserRole === 'OPERATOR'
						? '현재 처리할 운영 작업이 없습니다.'
						: '현재 진행 중인 길드 작업이 없습니다.'}
				</h2>
				<p class="mt-2 text-sm leading-6 text-slate-500">
					{guild.currentUserRole === 'OPERATOR'
						? '새 모집은 내전 또는 경매 운영 화면에서 시작할 수 있습니다.'
						: '완료된 내전은 상단 전적에서, 현재 시즌 순위는 리더보드에서 확인할 수 있습니다.'}
				</p>

				<div class="mt-5 flex flex-col justify-center gap-2 sm:flex-row">
					{#if guild.currentUserRole === 'OPERATOR'}
						<a
							href={seriesHref}
							class="inline-flex h-10 items-center justify-center gap-2 border border-cyan-300/30 bg-cyan-400/[0.065] px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
						>
							<Swords size={15} aria-hidden="true" /> 내전 운영
							<ArrowRight size={14} aria-hidden="true" />
						</a>
						<a
							href={auctionHref}
							class="inline-flex h-10 items-center justify-center gap-2 border border-emerald-300/25 bg-emerald-400/[0.055] px-4 text-xs font-semibold text-emerald-100 hover:bg-emerald-400/10"
						>
							<Gavel size={15} aria-hidden="true" /> 경매 운영
							<ArrowRight size={14} aria-hidden="true" />
						</a>
					{:else}
						<a
							href={leaderboardHref}
							class="inline-flex h-10 items-center justify-center gap-2 border border-cyan-300/30 bg-cyan-400/[0.065] px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
						>
							<Trophy size={15} aria-hidden="true" /> 리더보드 보기
							<ArrowRight size={14} aria-hidden="true" />
						</a>
					{/if}
				</div>
			</div>
		</section>
	{/if}
</div>
