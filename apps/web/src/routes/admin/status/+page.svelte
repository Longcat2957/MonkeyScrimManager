<script lang="ts">
	import { Activity, Bot, Clock3, Database, RefreshCw, Server, TriangleAlert } from 'lucide-svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';

	type ServiceStatus = 'normal' | 'delayed' | 'down';

	interface ServiceItem {
		name: string;
		description: string;
		status: ServiceStatus;
		statusLabel: string;
		value: string;
		icon: typeof Activity;
		lastNormal?: string;
		issue?: string;
	}

	const services: ServiceItem[] = [
		{
			name: 'API',
			description: '웹 요청 처리',
			status: 'normal',
			statusLabel: '정상',
			value: '42ms',
			icon: Server
		},
		{
			name: 'Database',
			description: '운영 데이터 연결',
			status: 'normal',
			statusLabel: '정상',
			value: '18ms',
			icon: Database
		},
		{
			name: 'Bot heartbeat',
			description: 'Discord Bot 연결',
			status: 'delayed',
			statusLabel: '지연',
			value: '1분 42초 전',
			icon: Bot,
			lastNormal: '14:35:38',
			issue: '마지막 heartbeat가 정상 기준 90초를 초과했습니다.'
		},
		{
			name: 'Valkey',
			description: '캐시 및 임시 상태',
			status: 'normal',
			statusLabel: '정상',
			value: '9ms',
			icon: Activity
		}
	];

	const issues = services.filter((service) => service.status !== 'normal');
	let checkedAt = $state(new Date('2026-07-11T14:37:20+09:00'));
	let refreshing = $state(false);

	const statusTone = {
		normal: 'success',
		delayed: 'warning',
		down: 'danger'
	} as const;

	const iconTone = {
		normal: 'border-emerald-400/25 bg-emerald-400/10 text-emerald-200',
		delayed: 'border-amber-300/30 bg-amber-300/10 text-amber-200',
		down: 'border-red-400/30 bg-red-500/10 text-red-200'
	};

	function formatCheckedAt(date: Date): string {
		return new Intl.DateTimeFormat('ko-KR', {
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit'
		}).format(date);
	}

	function serviceBorderClass(index: number): string {
		const mobile = index < services.length - 1 ? 'border-b' : '';
		const tabletRow = index < 2 ? 'sm:border-b' : 'sm:border-b-0';
		const tabletColumn = index % 2 === 0 ? 'sm:border-r' : 'sm:border-r-0';
		const desktopColumn = index < services.length - 1 ? 'xl:border-r' : 'xl:border-r-0';

		return `${mobile} ${tabletRow} ${tabletColumn} xl:border-b-0 ${desktopColumn}`;
	}

	async function refreshStatus() {
		if (refreshing) return;
		refreshing = true;
		await new Promise((resolve) => setTimeout(resolve, 450));
		checkedAt = new Date();
		refreshing = false;
	}
</script>

<svelte:head>
	<title>운영 상태 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="Admin"
		title="운영 상태"
		description="서비스 연결 상태를 확인합니다."
		metrics={[
			{
				label: '현재 상태',
				value: issues.length > 0 ? '점검 필요' : '정상',
				tone: issues.length > 0 ? 'warning' : 'success'
			},
			{ label: '문제', value: issues.length, tone: issues.length > 0 ? 'warning' : 'neutral' }
		]}
	>
		{#snippet actions()}
			<button
				type="button"
				onclick={refreshStatus}
				disabled={refreshing}
				class="inline-flex h-10 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100 disabled:cursor-wait disabled:opacity-60"
			>
				<RefreshCw size={15} class={refreshing ? 'animate-spin' : ''} aria-hidden="true" />
				새로고침
			</button>
		{/snippet}
	</WorkspaceHeader>

	<Panel compact>
		<div
			class="flex min-h-12 flex-col gap-2 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
		>
			<div class="flex items-center gap-2">
				{#if issues.length > 0}
					<TriangleAlert size={17} class="text-amber-200" aria-hidden="true" />
					<strong class="text-sm text-slate-100">확인이 필요한 서비스가 있습니다.</strong>
				{:else}
					<Activity size={17} class="text-emerald-200" aria-hidden="true" />
					<strong class="text-sm text-slate-100">모든 서비스가 정상입니다.</strong>
				{/if}
			</div>
			<p class="flex items-center gap-1.5 text-xs text-slate-500" aria-live="polite">
				<Clock3 size={13} aria-hidden="true" />
				마지막 확인 {formatCheckedAt(checkedAt)}
			</p>
		</div>

		<div class="grid sm:grid-cols-2 xl:grid-cols-4">
			{#each services as service, index (service.name)}
				<div
					class={`min-w-0 border-[var(--color-app-border-muted)] p-4 ${serviceBorderClass(index)}`}
				>
					<div class="flex items-start gap-3">
						<div
							class={`grid size-9 shrink-0 place-items-center border ${iconTone[service.status]}`}
						>
							<service.icon size={17} aria-hidden="true" />
						</div>
						<div class="min-w-0 flex-1">
							<div class="flex items-start justify-between gap-2">
								<div class="min-w-0">
									<h2 class="truncate text-sm font-semibold text-slate-100">{service.name}</h2>
									<p class="mt-0.5 truncate text-[11px] text-slate-500">{service.description}</p>
								</div>
								<StatusPill tone={statusTone[service.status]} label={service.statusLabel} />
							</div>
							<p class="mt-4 text-sm font-semibold tabular-nums text-slate-300">{service.value}</p>
						</div>
					</div>
				</div>
			{/each}
		</div>
	</Panel>

	{#if issues.length > 0}
		<Panel title="문제 상세" description="현재 점검이 필요한 항목만 표시합니다." compact>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each issues as service (service.name)}
					<div
						class="grid gap-3 px-4 py-4 md:grid-cols-[180px_minmax(0,1fr)_140px] md:items-center"
					>
						<div class="flex items-center gap-2">
							<TriangleAlert size={15} class="shrink-0 text-amber-200" aria-hidden="true" />
							<strong class="truncate text-sm text-slate-100">{service.name}</strong>
						</div>
						<p class="text-xs leading-5 text-slate-400">{service.issue}</p>
						<p class="text-xs text-slate-500 md:text-right">
							마지막 정상 <span class="font-semibold tabular-nums text-slate-300"
								>{service.lastNormal}</span
							>
						</p>
					</div>
				{/each}
			</div>
		</Panel>
	{/if}
</div>
