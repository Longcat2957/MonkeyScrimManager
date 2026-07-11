<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, UsersRound } from 'lucide-svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import RowList from '$lib/components/layout/RowList.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import type { MockRecruitmentQueueItem } from '$lib/fixtures/recruitments';

	interface Props {
		items: MockRecruitmentQueueItem[];
	}

	let { items }: Props = $props();

	function formatTime(unixSec: number): string {
		return new Intl.DateTimeFormat('ko-KR', {
			month: '2-digit',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit'
		}).format(unixSec * 1000);
	}
</script>

<Panel
	kicker="엔트리"
	title="엔트리 대기열"
	description="정원이 찬 일반 모집을 엔트리 편성 화면으로 이어갑니다."
	compact
>
	<RowList columns="minmax(0,1fr) 7rem auto" headings={['모집', '정원', '']}>
		{#each items as item (item.id)}
			<a
				href={resolve('/recruitments/[id]/entry', { id: String(item.id) })}
				class="group block px-4 py-3 hover:bg-[var(--color-app-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-300/50"
			>
				<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_7rem_auto] md:items-center">
					<div class="min-w-0">
						<div class="flex min-w-0 flex-wrap items-center gap-2">
							<UsersRound size={15} class="text-cyan-200" />
							<p class="truncate text-base font-semibold text-slate-100">모집 #{item.id}</p>
							<StatusPill label="엔트리 대기" tone="warning" />
						</div>
						<p class="mt-1 truncate text-xs font-semibold text-slate-500">
							정원 {item.targetCount} · 생성 {formatTime(item.createdAt)} · 생성자 {item.createdBy}
						</p>
					</div>

					<p class="font-mono text-lg font-bold tabular-nums text-slate-100 md:text-right">
						{item.targetCount}
					</p>

					<span
						class="inline-flex h-9 items-center justify-center gap-1.5 border border-cyan-400/35 px-3 text-xs font-semibold text-cyan-100 group-hover:bg-cyan-400/10"
					>
						엔트리 편집
						<ArrowRight size={14} />
					</span>
				</div>
			</a>
		{:else}
			<div class="grid min-h-36 place-items-center px-4 py-8 text-center">
				<div class="max-w-sm">
					<p class="text-sm font-semibold text-slate-300">엔트리 대기 모집 없음</p>
					<p class="mt-2 text-xs leading-5 text-slate-500">
						Discord에서 /내전모집을 시작하고 정원이 차면 여기에 표시됩니다.
					</p>
				</div>
			</div>
		{/each}
	</RowList>
</Panel>
