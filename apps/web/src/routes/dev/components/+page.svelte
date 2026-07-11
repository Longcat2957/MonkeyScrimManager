<script lang="ts">
	import { Activity, RadioTower } from 'lucide-svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import OperationsQueueBoard from '$lib/components/workflow/OperationsQueueBoard.svelte';
	import RealtimeEditStateBar from '$lib/components/workflow/RealtimeEditStateBar.svelte';
	import {
		mockOperationsQueue,
		type RealtimeConnectionState,
		type RealtimeSaveState
	} from '$lib/fixtures/componentDevelopment';

	type DemoMode = 'normal' | 'saving' | 'reconnecting' | 'offline' | 'conflict';

	let demoMode = $state<DemoMode>('normal');
	let realtimeNotice = $state<string | null>(null);

	const demoModes: Array<{ value: DemoMode; label: string }> = [
		{ value: 'normal', label: '정상' },
		{ value: 'saving', label: '저장 중' },
		{ value: 'reconnecting', label: '재연결' },
		{ value: 'offline', label: '오프라인' },
		{ value: 'conflict', label: '충돌' }
	];

	const connectionState: RealtimeConnectionState = $derived(
		demoMode === 'reconnecting' ? 'reconnecting' : demoMode === 'offline' ? 'offline' : 'connected'
	);
	const saveState: RealtimeSaveState = $derived(
		demoMode === 'saving' ? 'saving' : demoMode === 'conflict' ? 'conflict' : 'saved'
	);

	function selectDemoMode(mode: DemoMode) {
		demoMode = mode;
		realtimeNotice = null;
	}

	function reconnect() {
		demoMode = 'reconnecting';
		realtimeNotice = null;
		window.setTimeout(() => {
			demoMode = 'normal';
			realtimeNotice = '다시 연결됨';
		}, 700);
	}

	function resolveConflict(message: string) {
		demoMode = 'normal';
		realtimeNotice = message;
	}
</script>

<svelte:head>
	<title>컴포넌트 개발 | MonkeyBot</title>
</svelte:head>

<div class="space-y-8">
	<WorkspaceHeader
		kicker="개발"
		title="MVP 컴포넌트"
		description="현재 신규 개발 중인 운영 컴포넌트입니다."
		metrics={[{ label: '컴포넌트', value: 2, tone: 'info' }]}
	/>

	<section class="space-y-3">
		<div class="flex items-center gap-2 border-b border-[var(--color-app-border-muted)] pb-2">
			<Activity size={15} class="text-cyan-300" aria-hidden="true" />
			<h2 class="text-sm font-semibold text-slate-200">OperationsQueueBoard</h2>
		</div>
		<OperationsQueueBoard items={mockOperationsQueue} />
	</section>

	<section class="space-y-3">
		<div
			class="flex flex-col gap-3 border-b border-[var(--color-app-border-muted)] pb-2 sm:flex-row sm:items-center sm:justify-between"
		>
			<div class="flex items-center gap-2">
				<RadioTower size={15} class="text-cyan-300" aria-hidden="true" />
				<h2 class="text-sm font-semibold text-slate-200">RealtimeEditStateBar</h2>
			</div>
			<div
				class="flex max-w-full overflow-x-auto border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
				role="tablist"
				aria-label="실시간 상태 샘플"
			>
				{#each demoModes as mode (mode.value)}
					<button
						type="button"
						role="tab"
						aria-selected={demoMode === mode.value}
						onclick={() => selectDemoMode(mode.value)}
						class={`h-8 shrink-0 rounded-sm px-3 text-[11px] font-semibold ${
							demoMode === mode.value
								? 'bg-cyan-400/10 text-cyan-100'
								: 'text-slate-500 hover:text-slate-200'
						}`}
					>
						{mode.label}
					</button>
				{/each}
			</div>
		</div>

		<RealtimeEditStateBar
			resourceLabel="금요 정기 내전 · 2세트 픽밴"
			{connectionState}
			{saveState}
			lastSavedAt="14:52"
			editors={[
				{ id: 'me', name: '탐솔', isSelf: true },
				{ id: 'longcat', name: 'Longcat' }
			]}
			conflictMessage="Longcat이 2세트 블루팀 밴을 먼저 변경했습니다."
			onReconnect={reconnect}
			onAcceptServer={() => resolveConflict('최신 상태 반영됨')}
			onKeepLocal={() => resolveConflict('로컬 입력 유지됨')}
		/>

		{#if realtimeNotice}
			<p class="text-right text-xs font-semibold text-emerald-300" aria-live="polite">
				{realtimeNotice}
			</p>
		{/if}
	</section>
</div>
