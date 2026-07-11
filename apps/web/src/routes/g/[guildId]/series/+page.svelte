<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, CheckCircle2, Swords, Trophy, UsersRound } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const activeWork = $derived([
		{
			id: 'r-214',
			title: '모집 #214',
			format: '10인',
			status: '엔트리 편성',
			detail: '정원 10/10 · 모집 마감',
			updatedAt: '3분 전',
			href: resolve('/g/[guildId]/recruitments/[id]/entry', {
				guildId: data.guildId,
				id: '214'
			}),
			icon: UsersRound
		},
		{
			id: 's-106',
			title: '금요 정기 내전',
			format: 'BO3',
			status: '픽밴 입력',
			detail: '1세트 · 사이드 선택 대기',
			updatedAt: '8분 전',
			href: resolve('/g/[guildId]/series/[id]/pick-ban', {
				guildId: data.guildId,
				id: 's-106'
			}),
			icon: Swords
		},
		{
			id: 's-105',
			title: '주말 자유 내전',
			format: 'BO3',
			status: '결과 입력',
			detail: '2세트 · 결과 확인 대기',
			updatedAt: '21분 전',
			href: resolve('/g/[guildId]/series/[id]/result', {
				guildId: data.guildId,
				id: 's-105'
			}),
			icon: Swords
		}
	]);

	const completedSeries = [
		{
			id: 's-104',
			name: '금요 정기 내전',
			format: 'BO3',
			score: '2 : 1',
			winner: '1팀',
			playedAt: '3일 전'
		},
		{
			id: 's-103',
			name: '주말 자유 내전',
			format: 'BO3',
			score: '2 : 0',
			winner: '2팀',
			playedAt: '8일 전'
		},
		{
			id: 's-102',
			name: '수요 정기 내전',
			format: 'BO3',
			score: '1 : 2',
			winner: '2팀',
			playedAt: '12일 전'
		}
	];
</script>

<svelte:head>
	<title>내전 운영 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader kicker="일반 내전" title="내전 운영" description="모집 마감부터 완료 내전까지" />

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<Swords size={16} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">진행 중</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{activeWork.length}건</span>
		</header>
		<div
			class="hidden grid-cols-[minmax(220px,1fr)_90px_minmax(200px,1fr)_90px_116px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
		>
			<span>시리즈</span><span>포맷</span><span>현재 상태</span><span>갱신</span><span></span>
		</div>
		<div class="divide-y divide-[var(--color-app-border-muted)]">
			{#each activeWork as series (series.id)}
				<div
					class="grid gap-3 px-4 py-3 md:grid-cols-[minmax(220px,1fr)_90px_minmax(200px,1fr)_90px_116px] md:items-center"
				>
					<div class="flex min-w-0 items-center gap-3">
						<span
							class="grid size-9 shrink-0 place-items-center border border-cyan-400/25 bg-cyan-400/8 text-cyan-200"
						>
							<series.icon size={16} aria-hidden="true" />
						</span>
						<div class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{series.title}</strong>
							<small class="text-[10px] text-slate-600">#{series.id}</small>
						</div>
					</div>
					<span class="text-xs font-semibold text-slate-400">{series.format}</span>
					<div class="flex min-w-0 items-center gap-2">
						<StatusPill label={series.status} tone="warning" />
						<span class="truncate text-xs text-slate-500">{series.detail}</span>
					</div>
					<span class="text-xs text-slate-600">{series.updatedAt}</span>
					<a
						href={series.href}
						class="inline-flex h-9 items-center justify-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
					>
						작업 열기 <ArrowRight size={14} aria-hidden="true" />
					</a>
				</div>
			{/each}
		</div>
	</section>

	<section
		id="completed"
		class="scroll-mt-20 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<CheckCircle2 size={16} class="shrink-0 text-emerald-300" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">완료된 내전</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{completedSeries.length}건</span>
		</header>

		<div class="divide-y divide-[var(--color-app-border-muted)] md:hidden">
			{#each completedSeries as series (series.id)}
				<a
					href={resolve('/g/[guildId]/series/[id]/result', {
						guildId: data.guildId,
						id: series.id
					})}
					class="grid gap-3 p-3 hover:bg-[var(--color-app-hover)]"
				>
					<div class="flex min-w-0 items-start justify-between gap-3">
						<span class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{series.name}</strong>
							<small class="text-[10px] text-slate-600">#{series.id} · {series.playedAt}</small>
						</span>
						<ArrowRight size={15} class="shrink-0 text-slate-600" aria-hidden="true" />
					</div>
					<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
						<div class="bg-[var(--color-app-surface)] px-2 py-2">
							<span class="block text-[9px] font-semibold text-slate-600">포맷</span>
							<strong class="mt-0.5 block text-xs text-slate-300">{series.format}</strong>
						</div>
						<div class="bg-[var(--color-app-surface)] px-2 py-2 text-center">
							<span class="block text-[9px] font-semibold text-slate-600">스코어</span>
							<strong class="mt-0.5 block text-sm tabular-nums text-slate-100"
								>{series.score}</strong
							>
						</div>
						<div class="bg-[var(--color-app-surface)] px-2 py-2">
							<span class="block text-[9px] font-semibold text-slate-600">승리 팀</span>
							<strong class="mt-0.5 flex items-center gap-1 text-xs text-amber-200">
								<Trophy size={12} aria-hidden="true" />
								{series.winner}
							</strong>
						</div>
					</div>
				</a>
			{/each}
		</div>

		<div class="hidden md:block">
			<div
				class="grid grid-cols-[minmax(240px,1fr)_90px_110px_110px_90px_40px] items-center border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>시리즈</span><span>포맷</span><span class="text-center">스코어</span><span
					>승리 팀</span
				><span>완료</span><span></span>
			</div>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each completedSeries as series (series.id)}
					<a
						href={resolve('/g/[guildId]/series/[id]/result', {
							guildId: data.guildId,
							id: series.id
						})}
						class="grid min-h-16 grid-cols-[minmax(240px,1fr)_90px_110px_110px_90px_40px] items-center px-4 hover:bg-[var(--color-app-hover)]"
					>
						<span class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{series.name}</strong>
							<small class="text-[10px] text-slate-600">#{series.id}</small>
						</span>
						<span class="text-xs text-slate-500">{series.format}</span>
						<strong class="text-center text-base tabular-nums text-slate-100">{series.score}</strong
						>
						<span class="inline-flex items-center gap-1.5 text-xs font-semibold text-amber-200">
							<Trophy size={13} aria-hidden="true" />
							{series.winner}
						</span>
						<span class="text-xs text-slate-500">{series.playedAt}</span>
						<ArrowRight size={15} class="text-slate-600" aria-hidden="true" />
					</a>
				{/each}
			</div>
		</div>
	</section>
</div>
