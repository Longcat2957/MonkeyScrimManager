<script lang="ts">
	import { Gamepad2, Search, Sparkles, Timer } from 'lucide-svelte';
	import QuickActionFab from '$lib/components/fab/QuickActionFab.svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';

	const checks = [
		{ label: '하단 고정', value: '우하단' },
		{ label: '기능', value: '미니게임 / 전적검색 / 타이머' },
		{ label: '겹침', value: '전용 페이지에서 확인' }
	];
</script>

<div class="space-y-5 pb-28">
	<WorkspaceHeader
		kicker="FAB"
		title="빠른 실행 확인"
		description="미니게임, 전적검색, 타이머 Modal이 화면 전환 없이 자연스럽게 열리는지 확인하는 전용 화면입니다."
		metrics={[
			{ label: '위치', value: '하단', tone: 'info' },
			{ label: '상태', value: '분리', tone: 'success' },
			{ label: '내용', value: '비움', tone: 'neutral' }
		]}
	/>

	<div class="grid gap-5 xl:grid-cols-[minmax(0,1fr)_320px]">
		<Panel compact>
			<div class="grid gap-4 p-4">
				<div class="flex items-start gap-3">
					<div class="grid size-10 shrink-0 place-items-center bg-cyan-400/10 text-cyan-100">
						<Sparkles size={18} />
					</div>
					<div class="min-w-0">
						<p class="text-sm font-semibold text-slate-100">빠른 실행 버튼</p>
						<p class="mt-2 max-w-3xl text-sm leading-6 text-slate-400">
							이 화면에서는 FAB가 다른 컴포넌트와 겹치지 않는지, 모바일 폭에서 패널이 화면 밖으로
							나가지 않는지만 확인합니다.
						</p>
					</div>
				</div>

				<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-3">
					{#each checks as check (check.label)}
						<div class="bg-[var(--color-app-surface)] p-3">
							<p class="text-xs text-slate-500">{check.label}</p>
							<p class="mt-1 text-sm font-semibold text-slate-100">{check.value}</p>
						</div>
					{/each}
				</div>
			</div>
		</Panel>

		<Panel compact>
			<div class="grid gap-3 p-4">
				<div class="flex items-center justify-between gap-3">
					<div class="flex items-center gap-2">
						<Gamepad2 size={16} class="text-cyan-100" />
						<p class="text-sm font-semibold text-slate-100">미니게임</p>
					</div>
					<StatusPill label="Modal" tone="info" />
				</div>
				<div class="flex items-center justify-between gap-3">
					<div class="flex items-center gap-2">
						<Search size={16} class="text-amber-100" />
						<p class="text-sm font-semibold text-slate-100">전적검색</p>
					</div>
					<StatusPill label="Modal" tone="warning" />
				</div>
				<div class="flex items-center justify-between gap-3">
					<div class="flex items-center gap-2">
						<Timer size={16} class="text-emerald-100" />
						<p class="text-sm font-semibold text-slate-100">타이머</p>
					</div>
					<StatusPill label="Modal" tone="success" />
				</div>
			</div>
		</Panel>
	</div>
</div>

<QuickActionFab />
