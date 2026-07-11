<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, Trophy } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';

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
	<title>완료 시리즈 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="일반 내전"
		title="완료 시리즈"
		description="시리즈 결과 원장"
		metrics={[{ label: '완료', value: completedSeries.length, tone: 'success' }]}
	/>

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<div
			class="grid min-w-[720px] grid-cols-[minmax(240px,1fr)_90px_110px_110px_90px_40px] items-center border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
		>
			<span>시리즈</span><span>포맷</span><span class="text-center">스코어</span><span>승리 팀</span
			><span>완료</span><span></span>
		</div>
		<div class="overflow-x-auto">
			{#each completedSeries as series (series.id)}
				<a
					href={resolve('/series/[id]/result', { id: series.id })}
					class="grid min-h-16 min-w-[720px] grid-cols-[minmax(240px,1fr)_90px_110px_110px_90px_40px] items-center border-b border-[var(--color-app-border-muted)] px-4 hover:bg-[var(--color-app-hover)]"
				>
					<span class="min-w-0">
						<strong class="block truncate text-sm text-slate-100">{series.name}</strong>
						<small class="text-[10px] text-slate-600">#{series.id}</small>
					</span>
					<span class="text-xs text-slate-500">{series.format}</span>
					<strong class="text-center text-base tabular-nums text-slate-100">{series.score}</strong>
					<span class="inline-flex items-center gap-1.5 text-xs font-semibold text-amber-200">
						<Trophy size={13} aria-hidden="true" />
						{series.winner}
					</span>
					<span class="text-xs text-slate-500">{series.playedAt}</span>
					<ArrowRight size={15} class="text-slate-600" aria-hidden="true" />
				</a>
			{/each}
		</div>
	</section>
</div>
