<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, Gavel, Swords, Trophy, UsersRound } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const auctionWork = $derived([
		{
			id: 'ar10-42',
			title: '수요 10인 경매',
			kind: '모집',
			status: '팀장 설정',
			detail: '10명 · 2팀',
			href: resolve('/g/[guildId]/auction/recruitments/[id]/draft', {
				guildId: data.guildId,
				id: 'ar10-42'
			}),
			icon: UsersRound,
			tone: 'warning' as const
		},
		{
			id: 'ar20-42',
			title: '토요 20인 경매',
			kind: '모집',
			status: '팀장 설정',
			detail: '20명 · 4팀',
			href: resolve('/g/[guildId]/auction/recruitments/[id]/draft', {
				guildId: data.guildId,
				id: 'ar20-42'
			}),
			icon: UsersRound,
			tone: 'warning' as const
		},
		{
			id: 'a10-118',
			title: 'A10-118 경매 내전',
			kind: '토너먼트',
			status: '입찰 진행',
			detail: '2팀 · 4명 낙찰',
			href: resolve('/g/[guildId]/auction/tournaments/[id]/draft', {
				guildId: data.guildId,
				id: 'a10-118'
			}),
			icon: Gavel,
			tone: 'info' as const
		},
		{
			id: 'a20-118',
			title: 'A20-118 경매 내전',
			kind: '토너먼트',
			status: '입찰 진행',
			detail: '4팀 · 8명 낙찰',
			href: resolve('/g/[guildId]/auction/tournaments/[id]/draft', {
				guildId: data.guildId,
				id: 'a20-118'
			}),
			icon: Gavel,
			tone: 'info' as const
		},
		{
			id: 'a10-117',
			title: 'A10-117 경매 내전',
			kind: '토너먼트',
			status: '대진 진행',
			detail: '결승 · 라인업 대기',
			href: resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
				guildId: data.guildId,
				id: 'a10-117'
			}),
			icon: Swords,
			tone: 'warning' as const
		},
		{
			id: 'a20-117',
			title: 'A20-117 경매 내전',
			kind: '토너먼트',
			status: '대진 진행',
			detail: '준결승 A · 라인업 대기',
			href: resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
				guildId: data.guildId,
				id: 'a20-117'
			}),
			icon: Swords,
			tone: 'warning' as const
		},
		{
			id: 'a10-115',
			title: 'A10-115 경매 내전',
			kind: '토너먼트',
			status: '결과 입력',
			detail: '결승 · 1세트 결과 대기',
			href: `${resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
				guildId: data.guildId,
				id: 'a10-115'
			})}?view=result`,
			icon: Swords,
			tone: 'warning' as const
		},
		{
			id: 'a20-115',
			title: 'A20-115 경매 내전',
			kind: '토너먼트',
			status: '결과 입력',
			detail: '결승 · 1세트 결과 대기',
			href: `${resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
				guildId: data.guildId,
				id: 'a20-115'
			})}?view=result`,
			icon: Swords,
			tone: 'warning' as const
		},
		{
			id: 'a10-116',
			title: 'A10-116 경매 내전',
			kind: '완료',
			status: '결과 확정',
			detail: '2팀 · 우승 2팀',
			href: resolve('/g/[guildId]/auction/tournaments/[id]/result', {
				guildId: data.guildId,
				id: 'a10-116'
			}),
			icon: Trophy,
			tone: 'success' as const
		},
		{
			id: 'a20-116',
			title: 'A20-116 경매 내전',
			kind: '완료',
			status: '결과 확정',
			detail: '4팀 · 우승 2팀',
			href: resolve('/g/[guildId]/auction/tournaments/[id]/result', {
				guildId: data.guildId,
				id: 'a20-116'
			}),
			icon: Trophy,
			tone: 'success' as const
		}
	]);
	const activeAuctionWork = $derived(auctionWork.filter((item) => item.kind !== '완료'));
	const completedAuctionWork = $derived(auctionWork.filter((item) => item.kind === '완료'));
</script>

{#snippet AuctionRow(item: (typeof auctionWork)[number], actionLabel: string)}
	<div
		class="grid gap-3 px-4 py-3 md:grid-cols-[minmax(220px,1fr)_100px_minmax(180px,1fr)_116px] md:items-center"
	>
		<div class="flex min-w-0 items-center gap-3">
			<span
				class="grid size-9 shrink-0 place-items-center border border-cyan-400/25 bg-cyan-400/8 text-cyan-200"
			>
				<item.icon size={16} aria-hidden="true" />
			</span>
			<div class="min-w-0">
				<strong class="block truncate text-sm text-slate-100">{item.title}</strong>
				<small class="text-[10px] text-slate-600">#{item.id}</small>
			</div>
		</div>
		<span class="text-xs text-slate-500">{item.kind}</span>
		<div class="flex min-w-0 items-center gap-2">
			<StatusPill label={item.status} tone={item.tone} />
			<span class="truncate text-xs text-slate-500">{item.detail}</span>
		</div>
		<a
			href={item.href}
			class="inline-flex h-9 items-center justify-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
		>
			{actionLabel}
			<ArrowRight size={14} aria-hidden="true" />
		</a>
	</div>
{/snippet}

<svelte:head>
	<title>경매 내전 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader kicker="경매 내전" title="경매 작업" description="모집부터 완료 경매까지" />

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<Gavel size={16} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">진행 중</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{activeAuctionWork.length}건</span>
		</header>
		<div
			class="hidden grid-cols-[minmax(220px,1fr)_100px_minmax(180px,1fr)_116px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
		>
			<span>경매</span><span>구분</span><span>현재 상태</span><span></span>
		</div>
		<div class="divide-y divide-[var(--color-app-border-muted)]">
			{#each activeAuctionWork as item (item.id)}
				{@render AuctionRow(item, '작업 열기')}
			{/each}
		</div>
	</section>

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<Trophy size={16} class="shrink-0 text-emerald-300" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">완료된 경매</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{completedAuctionWork.length}건</span>
		</header>
		<div
			class="hidden grid-cols-[minmax(220px,1fr)_100px_minmax(180px,1fr)_116px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
		>
			<span>경매</span><span>구분</span><span>결과</span><span></span>
		</div>
		<div class="divide-y divide-[var(--color-app-border-muted)]">
			{#each completedAuctionWork as item (item.id)}
				{@render AuctionRow(item, '결과 보기')}
			{/each}
		</div>
	</section>
</div>
