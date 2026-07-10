<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, LayoutDashboard, Sparkles } from 'lucide-svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="홈"
		title="공통 화면 확인"
		description="상단 메뉴, 로고 홈 이동, 아바타 메뉴, 접히는 검색 패널이 실제 기능 화면 없이도 자연스럽게 보이는지 확인하는 첫 화면입니다."
		metrics={[
			{ label: '화면 구성', value: '상단 메뉴', tone: 'info' },
			{ label: '테마', value: '다크', tone: 'neutral' },
			{ label: '글꼴', value: 'Noto KR', tone: 'success' }
		]}
	/>

	<Panel kicker="공통 화면" title="기능 화면은 따로 확인" compact>
		<div class="grid gap-4 p-4 lg:grid-cols-[minmax(0,1fr)_260px]">
			<div class="min-w-0">
				<div class="flex items-start gap-3">
					<div
						class="grid size-10 shrink-0 place-items-center border border-cyan-400/40 bg-cyan-400/10"
					>
						<LayoutDashboard size={18} class="text-cyan-200" />
					</div>
					<div class="min-w-0">
						<p class="text-sm font-semibold text-slate-100">공통 화면 틀</p>
						<p class="mt-2 max-w-3xl text-sm leading-6 text-slate-400">
							첫 화면에서는 전체 화면의 밀도, 상단 메뉴 높이, 본문 폭만 가볍게 확인합니다.
						</p>
					</div>
				</div>
			</div>

			<div
				class="grid gap-2 border-l border-[var(--color-app-border-muted)] pl-4 max-lg:border-l-0 max-lg:pl-0"
			>
				<div class="flex items-center justify-between gap-3">
					<span class="text-xs text-slate-500">컴포넌트 점검</span>
					<StatusPill label="분리됨" tone="success" />
				</div>
				<a
					href={resolve('/dev/components')}
					class="inline-flex h-9 items-center justify-between gap-2 rounded-sm border border-[var(--color-app-border-muted)] px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100"
				>
					<span>컴포넌트 보기</span>
					<ArrowRight size={14} />
				</a>
				<a
					href={resolve('/dev/fab')}
					class="inline-flex h-9 items-center justify-between gap-2 rounded-sm border border-[var(--color-app-border-muted)] px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100"
				>
					<span class="inline-flex min-w-0 items-center gap-2">
						<Sparkles size={14} />
						FAB 보기
					</span>
					<ArrowRight size={14} />
				</a>
			</div>
		</div>
	</Panel>
</div>
