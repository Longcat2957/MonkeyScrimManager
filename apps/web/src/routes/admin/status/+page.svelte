<script lang="ts">
	import { onMount } from 'svelte';
	import {
		Activity,
		Bot,
		CheckCircle2,
		Clock3,
		Database,
		RefreshCw,
		Server,
		ShieldAlert,
		Trash2,
		TriangleAlert
	} from 'lucide-svelte';
	import {
		fetchServiceStatus,
		serviceStatusErrorText,
		type ServiceStatusSnapshot
	} from '$lib/api/serviceStatus';
	import {
		fetchGuildRecordResetPreview,
		resetGuildRecords,
		serviceAdminResetErrorText,
		type GuildRecordResetPreview,
		type GuildRecordResetResult
	} from '$lib/api/serviceAdminReset';
	import Panel from '$lib/components/layout/Panel.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';

	type ServiceStatus = 'normal' | 'delayed' | 'down';
	type ServiceKey = ServiceStatusSnapshot['services'][number]['key'];

	interface ServiceItem {
		key: ServiceKey;
		name: string;
		description: string;
		status: ServiceStatus;
		statusLabel: string;
		value: string;
		latencyMs: number | null;
		icon: typeof Activity;
		lastHealthyAt: string | null;
		issue: string | null;
	}

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

	const presentation: Record<ServiceKey, Pick<ServiceItem, 'name' | 'description' | 'icon'>> = {
		API: { name: 'API', description: '웹 요청 처리', icon: Server },
		POSTGRESQL: { name: 'Database', description: '운영 데이터 연결', icon: Database },
		BOT_HEARTBEAT: { name: 'Bot heartbeat', description: 'Discord Bot 연결', icon: Bot },
		VALKEY: { name: 'Valkey', description: '캐시 및 임시 상태', icon: Activity }
	};

	let snapshot = $state<ServiceStatusSnapshot | null>(null);
	let loading = $state(true);
	let refreshing = $state(false);
	let refreshError = $state<string | null>(null);
	let resetGuildId = $state('');
	let resetPreview = $state<GuildRecordResetPreview | null>(null);
	let resetResult = $state<GuildRecordResetResult | null>(null);
	let resetConfirmation = $state('');
	let resetPreviewing = $state(false);
	let resetting = $state(false);
	let resetError = $state<string | null>(null);
	let resetIdempotencyKey = $state<string | null>(null);

	const services = $derived(snapshot?.services.map(toServiceItem) ?? []);
	const issues = $derived(services.filter((service) => service.status !== 'normal'));
	const checkedAt = $derived(snapshot ? new Date(snapshot.checkedAt) : null);
	const canReset = $derived(
		resetPreview !== null &&
			resetConfirmation === resetPreview.confirmation &&
			!resetting &&
			!resetPreviewing
	);

	onMount(() => {
		void refreshStatus();
	});

	function toServiceItem(service: ServiceStatusSnapshot['services'][number]): ServiceItem {
		return {
			...presentation[service.key],
			...service,
			statusLabel: statusLabel(service.status)
		};
	}

	function statusLabel(status: ServiceStatus): string {
		if (status === 'normal') return '정상';
		if (status === 'delayed') return '지연';
		return '중단';
	}

	function formatCheckedAt(date: Date | null): string {
		if (!date || Number.isNaN(date.getTime())) return '확인 시간 없음';
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

	function formatValue(service: ServiceItem): string {
		return service.latencyMs === null ? service.value : `${service.value} · ${service.latencyMs}ms`;
	}

	async function refreshStatus() {
		if (refreshing) return;
		refreshing = true;
		refreshError = null;
		try {
			snapshot = await fetchServiceStatus();
		} catch (cause) {
			refreshError = serviceStatusErrorText(cause);
		} finally {
			loading = false;
			refreshing = false;
		}
	}

	async function inspectResetTarget() {
		if (resetPreviewing || resetting) return;
		resetPreviewing = true;
		resetError = null;
		resetPreview = null;
		resetResult = null;
		resetConfirmation = '';
		resetIdempotencyKey = null;
		try {
			resetPreview = await fetchGuildRecordResetPreview(resetGuildId.trim());
		} catch (cause) {
			resetError = serviceAdminResetErrorText(cause);
		} finally {
			resetPreviewing = false;
		}
	}

	async function executeReset() {
		if (!canReset || !resetPreview) return;
		resetting = true;
		resetError = null;
		const preview = resetPreview;
		resetIdempotencyKey ??= globalThis.crypto.randomUUID();
		try {
			resetResult = await resetGuildRecords(
				preview.guildId,
				resetConfirmation,
				resetIdempotencyKey
			);
			resetPreview = null;
			resetConfirmation = '';
			resetIdempotencyKey = null;
		} catch (cause) {
			resetError = serviceAdminResetErrorText(cause);
		} finally {
			resetting = false;
		}
	}
</script>

<svelte:head>
	<title>운영 상태 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader kicker="Admin" title="운영 상태" description="서비스 연결 상태를 확인합니다.">
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

	{#if snapshot}
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
				{#each services as service, index (service.key)}
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
								<p class="mt-4 text-sm font-semibold tabular-nums text-slate-300">
									{formatValue(service)}
								</p>
							</div>
						</div>
					</div>
				{/each}
			</div>
		</Panel>
	{:else if loading}
		<Panel compact>
			<div
				class="flex min-h-36 items-center justify-center px-4 text-sm text-slate-400"
				aria-busy="true"
			>
				운영 상태를 불러오는 중입니다.
			</div>
		</Panel>
	{/if}

	{#if refreshError}
		<Panel title="운영 상태를 불러오지 못했습니다." compact>
			<div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
				<p class="text-sm leading-6 text-slate-400">{refreshError}</p>
				<button
					type="button"
					onclick={refreshStatus}
					disabled={refreshing}
					class="inline-flex h-9 shrink-0 items-center justify-center border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100 disabled:opacity-60"
				>
					다시 시도
				</button>
			</div>
		</Panel>
	{/if}

	{#if issues.length > 0}
		<Panel title="문제 상세" description="현재 점검이 필요한 항목만 표시합니다." compact>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each issues as service (service.key)}
					<div
						class="grid gap-3 px-4 py-4 md:grid-cols-[180px_minmax(0,1fr)_170px] md:items-center"
					>
						<div class="flex items-center gap-2">
							<TriangleAlert size={15} class="shrink-0 text-amber-200" aria-hidden="true" />
							<strong class="truncate text-sm text-slate-100">{service.name}</strong>
						</div>
						<p class="text-xs leading-5 text-slate-400">
							{service.issue ?? '상태를 확인해야 합니다.'}
						</p>
						<p class="text-xs text-slate-500 md:text-right">
							마지막 정상 <span class="font-semibold tabular-nums text-slate-300"
								>{formatCheckedAt(
									service.lastHealthyAt ? new Date(service.lastHealthyAt) : null
								)}</span
							>
						</p>
					</div>
				{/each}
			</div>
		</Panel>
	{/if}

	<Panel
		title="테스트 데이터 초기화"
		description="서비스 관리자만 사용할 수 있는 개발 환경 전용 기능입니다."
		compact
	>
		<div class="border-b border-red-400/20 bg-red-500/[0.06] px-4 py-4">
			<div class="flex items-start gap-3">
				<div
					class="grid size-9 shrink-0 place-items-center border border-red-300/30 bg-red-500/10 text-red-100"
				>
					<ShieldAlert size={17} aria-hidden="true" />
				</div>
				<div class="min-w-0">
					<strong class="text-sm text-red-50"
						>선택한 서버의 모든 경기 이력을 영구 삭제합니다.</strong
					>
					<p class="mt-1 text-xs leading-5 text-red-100/70">
						시즌·모집·시리즈·게임·MMR·경매·가져오기 이력을 비우고 빈 활성 시즌을 만듭니다. 서버
						멤버, 설정, Discord 카탈로그와 Riot 계정은 유지됩니다. 이미 전송된 Discord 메시지는
						삭제하지 않습니다.
					</p>
				</div>
			</div>
		</div>

		<div class="space-y-4 px-4 py-4">
			<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
				<label class="grid gap-1.5">
					<span class="text-xs font-semibold text-slate-300">Discord 서버 ID</span>
					<input
						bind:value={resetGuildId}
						inputmode="numeric"
						autocomplete="off"
						placeholder="1525851181157580861"
						disabled={resetPreviewing || resetting}
						class="h-10 w-full border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm tabular-nums text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-400/50 disabled:opacity-50"
					/>
				</label>
				<button
					type="button"
					onclick={inspectResetTarget}
					disabled={resetGuildId.trim().length < 10 || resetPreviewing || resetting}
					class="inline-flex h-10 items-center justify-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-4 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100 disabled:cursor-not-allowed disabled:opacity-40"
				>
					<RefreshCw size={14} class={resetPreviewing ? 'animate-spin' : ''} aria-hidden="true" />
					{resetPreviewing ? '확인 중' : '초기화 대상 확인'}
				</button>
			</div>

			{#if resetError}
				<div
					class="border border-red-400/25 bg-red-500/[0.07] px-3 py-2.5 text-xs leading-5 text-red-100"
					role="alert"
				>
					{resetError}
				</div>
			{/if}

			{#if resetResult}
				<div
					class="border border-emerald-400/25 bg-emerald-500/[0.07] px-3 py-3"
					aria-live="polite"
				>
					<div class="flex items-center gap-2 text-emerald-100">
						<CheckCircle2 size={16} aria-hidden="true" />
						<strong class="text-sm">{resetResult.displayName} 초기화 완료</strong>
					</div>
					<p class="mt-1.5 text-xs text-emerald-100/70">
						{resetResult.deleted.totalRows.toLocaleString()}개 row를 삭제하고 ‘{resetResult
							.freshSeason.name}’ 빈 시즌을 만들었습니다.
					</p>
				</div>
			{/if}

			{#if resetPreview}
				<div class="border border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)]">
					<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
						<strong class="block text-sm text-slate-100">{resetPreview.displayName}</strong>
						<p class="mt-1 text-xs tabular-nums text-slate-500">
							{resetPreview.guildId} · 활성 멤버 {resetPreview.memberCount.toLocaleString()}명 ·
							{resetPreview.activeSeasonName}
						</p>
					</div>
					<dl
						class="grid grid-cols-2 divide-x divide-y divide-[var(--color-app-border-muted)] sm:grid-cols-4"
					>
						<div class="px-3 py-2.5">
							<dt class="text-[11px] text-slate-500">전체 삭제 row</dt>
							<dd class="mt-1 text-sm font-semibold tabular-nums text-slate-200">
								{resetPreview.counts.totalRows.toLocaleString()}
							</dd>
						</div>
						<div class="px-3 py-2.5">
							<dt class="text-[11px] text-slate-500">모집 / 시리즈</dt>
							<dd class="mt-1 text-sm font-semibold tabular-nums text-slate-200">
								{resetPreview.counts.recruitments.toLocaleString()} / {resetPreview.counts.series.toLocaleString()}
							</dd>
						</div>
						<div class="px-3 py-2.5">
							<dt class="text-[11px] text-slate-500">게임 / MMR row</dt>
							<dd class="mt-1 text-sm font-semibold tabular-nums text-slate-200">
								{resetPreview.counts.games.toLocaleString()} / {resetPreview.counts.mmrRows.toLocaleString()}
							</dd>
						</div>
						<div class="px-3 py-2.5">
							<dt class="text-[11px] text-slate-500">경매 대회 / import</dt>
							<dd class="mt-1 text-sm font-semibold tabular-nums text-slate-200">
								{resetPreview.counts.auctionTournaments.toLocaleString()} / {resetPreview.counts.importJobs.toLocaleString()}
							</dd>
						</div>
					</dl>
				</div>

				<label class="grid gap-1.5">
					<span class="text-xs font-semibold text-slate-300">
						계속하려면 <code class="select-all text-red-200">{resetPreview.confirmation}</code> 입력
					</span>
					<input
						bind:value={resetConfirmation}
						autocomplete="off"
						spellcheck="false"
						disabled={resetting}
						class="h-10 w-full border border-red-300/25 bg-red-950/20 px-3 font-mono text-sm text-red-50 outline-none placeholder:text-red-200/25 focus:border-red-300/55 disabled:opacity-50"
					/>
				</label>
				<button
					type="button"
					onclick={executeReset}
					disabled={!canReset}
					class="inline-flex h-10 w-full items-center justify-center gap-2 border border-red-300/35 bg-red-500/10 px-4 text-xs font-semibold text-red-50 hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-35 sm:w-auto"
				>
					<Trash2 size={15} aria-hidden="true" />
					{resetting ? '전체 기록 삭제 중' : '전체 기록 삭제'}
				</button>
			{/if}
		</div>
	</Panel>
</div>
