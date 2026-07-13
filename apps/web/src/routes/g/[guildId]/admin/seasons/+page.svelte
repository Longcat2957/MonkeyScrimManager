<script lang="ts">
	import { Dialog } from 'bits-ui';
	import {
		CalendarRange,
		Check,
		ChevronDown,
		Clock,
		Gavel,
		History,
		Plus,
		Swords,
		TriangleAlert,
		UsersRound,
		X
	} from 'lucide-svelte';
	import {
		fetchGuildAdminSeasons,
		GuildAdminSeasonsApiError,
		startGuildAdminSeason
	} from '$lib/api/guildAdminSeasons';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import type { GuildAdminSeasonSummary } from '$lib/contracts/workflow';
	import { untrack } from 'svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const initialSnapshot = untrack(() => data.seasonSnapshot);
	let seasonSnapshot = $state(initialSnapshot);
	let expandedSeasonId = $state<string | null>(initialSnapshot.completed[0]?.id ?? null);
	let startDialogOpen = $state(false);
	let newSeasonName = $state('');
	let notice = $state('');
	let startError = $state('');
	let submitting = $state(false);
	let mutationIntent = $state('');
	let mutationIdempotencyKey = $state('');
	let seasonNameInput: globalThis.HTMLInputElement | undefined = $state();

	const currentSeason = $derived(seasonSnapshot.current);
	const completedSeasons = $derived(seasonSnapshot.completed);
	const canStartSeason = $derived(
		newSeasonName.trim().length > 0 && [...newSeasonName.trim()].length <= 100
	);
	const renderedAt = new Date();
	const seoulDateFormatter = new Intl.DateTimeFormat('en-CA', {
		timeZone: 'Asia/Seoul',
		year: 'numeric',
		month: '2-digit',
		day: '2-digit'
	});
	const seoulDateTimeFormatter = new Intl.DateTimeFormat('en-CA', {
		timeZone: 'Asia/Seoul',
		year: 'numeric',
		month: '2-digit',
		day: '2-digit',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});

	function toggleSeason(seasonId: string) {
		expandedSeasonId = expandedSeasonId === seasonId ? null : seasonId;
	}

	function prepareStartDialog() {
		newSeasonName = '';
		startError = '';
	}

	function focusSeasonName(event: globalThis.Event) {
		event.preventDefault();
		seasonNameInput?.focus();
	}

	function preventCloseWhileSubmitting(event: globalThis.Event) {
		if (submitting) event.preventDefault();
	}

	function clearStartError() {
		startError = '';
	}

	async function startSeason(event: globalThis.SubmitEvent) {
		event.preventDefault();
		const name = newSeasonName.trim();
		if (!canStartSeason || !name || submitting) return;

		const expectedCurrentSeasonId = currentSeason.id;
		const intent = JSON.stringify([name, expectedCurrentSeasonId]);
		if (intent !== mutationIntent) {
			mutationIntent = intent;
			mutationIdempotencyKey = globalThis.crypto.randomUUID();
		}

		submitting = true;
		startError = '';
		try {
			const authoritative = await startGuildAdminSeason(data.guildId, {
				name,
				expectedCurrentSeasonId,
				idempotencyKey: mutationIdempotencyKey
			});
			seasonSnapshot = authoritative;
			expandedSeasonId = authoritative.completed[0]?.id ?? null;
			notice = `${authoritative.current.name} 시작됨`;
			startDialogOpen = false;
			newSeasonName = '';
		} catch (cause) {
			let message = errorText(cause);
			if (
				cause instanceof GuildAdminSeasonsApiError &&
				cause.status === 409 &&
				cause.code === 'current_season_conflict'
			) {
				try {
					const authoritative = await fetchGuildAdminSeasons(data.guildId);
					seasonSnapshot = authoritative;
					expandedSeasonId = authoritative.completed[0]?.id ?? null;
				} catch (refreshCause) {
					message = `${message} ${errorText(refreshCause)}`;
				}
			}
			startError = message;
		} finally {
			submitting = false;
		}
	}

	function errorText(cause: unknown): string {
		return cause instanceof Error ? cause.message : '새 시즌 시작 요청을 처리하지 못했습니다.';
	}

	function formatSeoulDate(value: string | Date): string {
		const parts = dateParts(seoulDateFormatter, value);
		return `${parts.year}. ${parts.month}. ${parts.day}.`;
	}

	function formatSeoulDateTime(value: string): string {
		const parts = dateParts(seoulDateTimeFormatter, value);
		return `${parts.year}. ${parts.month}. ${parts.day}. ${parts.hour}:${parts.minute}`;
	}

	function formatDuration(season: GuildAdminSeasonSummary): string {
		const start = seoulDayTimestamp(season.startedAt);
		const end = seoulDayTimestamp(season.endedAt ?? renderedAt);
		const days = Math.max(1, Math.floor((end - start) / 86_400_000) + 1);
		return `${days}일${season.status === 'ACTIVE' ? '째' : ''}`;
	}

	function formatPeriod(season: GuildAdminSeasonSummary): string {
		const end = season.endedAt ? formatSeoulDate(season.endedAt) : '진행 중';
		return `${formatSeoulDate(season.startedAt)} - ${end} · ${formatDuration(season)}`;
	}

	function seoulDayTimestamp(value: string | Date): number {
		const parts = dateParts(seoulDateFormatter, value);
		return Date.UTC(Number(parts.year), Number(parts.month) - 1, Number(parts.day));
	}

	function dateParts(formatter: Intl.DateTimeFormat, value: string | Date) {
		const formatted = formatter.formatToParts(typeof value === 'string' ? new Date(value) : value);
		const part = (type: Intl.DateTimeFormatPartTypes) =>
			formatted.find((candidate) => candidate.type === type)?.value ?? '';
		return {
			year: part('year'),
			month: part('month'),
			day: part('day'),
			hour: part('hour'),
			minute: part('minute')
		};
	}
</script>

<Dialog.Root bind:open={startDialogOpen}>
	<div class="space-y-4">
		<section
			class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
		>
			<header
				class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
			>
				<div class="flex min-w-0 items-center gap-3">
					<CalendarRange size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
					<div class="min-w-0">
						<h2 class="truncate text-sm font-semibold text-slate-50">현재 시즌</h2>
						<p class="text-xs text-slate-500">길드 전적과 MMR 집계 기간</p>
					</div>
				</div>
				<div
					class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center sm:justify-end"
				>
					{#if notice}
						<span
							class="inline-flex min-w-0 items-center gap-1 text-xs font-semibold text-emerald-300"
							role="status"
						>
							<Check size={14} class="shrink-0" aria-hidden="true" />
							<span class="truncate">{notice}</span>
						</span>
					{/if}
					<Dialog.Trigger
						type="button"
						disabled={submitting}
						onclick={prepareStartDialog}
						class="inline-flex h-9 w-full shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35 sm:w-auto"
					>
						<Plus size={14} aria-hidden="true" />
						새 시즌 시작
					</Dialog.Trigger>
				</div>
			</header>

			<div
				class="grid gap-4 border-b border-[var(--color-app-border-muted)] px-4 py-5 lg:grid-cols-[minmax(0,1.2fr)_minmax(28rem,1fr)] lg:items-end"
			>
				<div class="min-w-0">
					<div class="flex flex-wrap items-center gap-2">
						<h3 class="min-w-0 truncate text-xl font-semibold text-slate-50">
							{currentSeason.name}
						</h3>
						<StatusPill label="진행 중" tone="success" />
					</div>
					<p class="mt-2 text-xs text-slate-500">
						{formatSeoulDate(currentSeason.startedAt)} 시작 · {formatDuration(currentSeason)}
					</p>
				</div>
				<dl class="grid grid-cols-1 gap-3 text-xs sm:grid-cols-3">
					<div class="min-w-0">
						<dt class="text-[10px] font-semibold text-slate-600">시작한 운영자</dt>
						<dd class="mt-1 truncate font-medium text-slate-300">
							{currentSeason.createdBy ?? '시스템'}
						</dd>
					</div>
					<div class="min-w-0">
						<dt class="text-[10px] font-semibold text-slate-600">최근 확정 경기</dt>
						<dd class="mt-1 truncate font-medium text-slate-300">
							{currentSeason.lastGameAt
								? formatSeoulDateTime(currentSeason.lastGameAt)
								: '아직 없음'}
						</dd>
					</div>
					<div class="min-w-0">
						<dt class="text-[10px] font-semibold text-slate-600">경매 구성</dt>
						<dd class="mt-1 truncate font-medium text-slate-300">
							10인 {currentSeason.auctionTenCount} · 20인 {currentSeason.auctionTwentyCount}
						</dd>
					</div>
				</dl>
			</div>

			<dl class="grid grid-cols-2 gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-4">
				<div class="bg-[var(--color-app-bg-soft)] px-4 py-3">
					<dt class="flex items-center gap-1.5 text-[10px] font-semibold text-slate-600">
						<UsersRound size={13} aria-hidden="true" /> 참가자
					</dt>
					<dd class="mt-1 font-mono text-lg font-semibold text-slate-100">
						{currentSeason.participantCount}<span class="ml-1 font-sans text-xs text-slate-500"
							>명</span
						>
					</dd>
				</div>
				<div class="bg-[var(--color-app-bg-soft)] px-4 py-3">
					<dt class="flex items-center gap-1.5 text-[10px] font-semibold text-slate-600">
						<Swords size={13} aria-hidden="true" /> 일반 시리즈
					</dt>
					<dd class="mt-1 font-mono text-lg font-semibold text-slate-100">
						{currentSeason.generalSeriesCount}<span class="ml-1 font-sans text-xs text-slate-500"
							>회</span
						>
					</dd>
				</div>
				<div class="bg-[var(--color-app-bg-soft)] px-4 py-3">
					<dt class="flex items-center gap-1.5 text-[10px] font-semibold text-slate-600">
						<Gavel size={13} aria-hidden="true" /> 경매 내전
					</dt>
					<dd class="mt-1 font-mono text-lg font-semibold text-slate-100">
						{currentSeason.auctionTournamentCount}<span
							class="ml-1 font-sans text-xs text-slate-500">회</span
						>
					</dd>
				</div>
				<div class="bg-[var(--color-app-bg-soft)] px-4 py-3">
					<dt class="flex items-center gap-1.5 text-[10px] font-semibold text-slate-600">
						<Clock size={13} aria-hidden="true" /> 확정 게임
					</dt>
					<dd class="mt-1 font-mono text-lg font-semibold text-slate-100">
						{currentSeason.confirmedGameCount}<span class="ml-1 font-sans text-xs text-slate-500"
							>게임</span
						>
					</dd>
				</div>
			</dl>
		</section>

		<section
			class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
		>
			<header
				class="flex min-h-14 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
			>
				<div class="flex min-w-0 items-center gap-3">
					<History size={18} class="shrink-0 text-slate-400" aria-hidden="true" />
					<div class="min-w-0">
						<h2 class="truncate text-sm font-semibold text-slate-50">지난 시즌</h2>
						<p class="text-xs text-slate-500">종료된 시즌 기록</p>
					</div>
				</div>
				<span class="shrink-0 text-xs font-semibold text-slate-500"
					>{completedSeasons.length}개</span
				>
			</header>

			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each completedSeasons as season (season.id)}
					<div>
						<button
							type="button"
							onclick={() => toggleSeason(season.id)}
							aria-expanded={expandedSeasonId === season.id}
							aria-controls={`season-detail-${season.id}`}
							class="flex w-full items-center justify-between gap-4 px-4 py-3.5 text-left hover:bg-[var(--color-app-hover)]"
						>
							<span class="min-w-0">
								<strong class="block truncate text-sm font-semibold text-slate-200"
									>{season.name}</strong
								>
								<small class="mt-1 block truncate text-[11px] text-slate-500">
									{formatPeriod(season)}
								</small>
							</span>
							<ChevronDown
								size={16}
								class={`shrink-0 text-slate-500 transition ${expandedSeasonId === season.id ? 'rotate-180' : ''}`}
								aria-hidden="true"
							/>
						</button>

						{#if expandedSeasonId === season.id}
							<div
								id={`season-detail-${season.id}`}
								class="border-t border-[var(--color-app-border-muted)] bg-slate-950/20"
							>
								<dl
									class="grid grid-cols-2 gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-4"
								>
									<div class="bg-[var(--color-app-surface)] px-4 py-3">
										<dt class="text-[10px] font-semibold text-slate-600">참가자</dt>
										<dd class="mt-1 font-mono text-base font-semibold text-slate-200">
											{season.participantCount}<span
												class="ml-1 font-sans text-[11px] text-slate-500">명</span
											>
										</dd>
									</div>
									<div class="bg-[var(--color-app-surface)] px-4 py-3">
										<dt class="text-[10px] font-semibold text-slate-600">일반 시리즈</dt>
										<dd class="mt-1 font-mono text-base font-semibold text-slate-200">
											{season.generalSeriesCount}<span
												class="ml-1 font-sans text-[11px] text-slate-500">회</span
											>
										</dd>
									</div>
									<div class="bg-[var(--color-app-surface)] px-4 py-3">
										<dt class="text-[10px] font-semibold text-slate-600">경매 내전</dt>
										<dd class="mt-1 font-mono text-base font-semibold text-slate-200">
											{season.auctionTournamentCount}<span
												class="ml-1 font-sans text-[11px] text-slate-500">회</span
											>
										</dd>
									</div>
									<div class="bg-[var(--color-app-surface)] px-4 py-3">
										<dt class="text-[10px] font-semibold text-slate-600">확정 게임</dt>
										<dd class="mt-1 font-mono text-base font-semibold text-slate-200">
											{season.confirmedGameCount}<span
												class="ml-1 font-sans text-[11px] text-slate-500">게임</span
											>
										</dd>
									</div>
								</dl>
								<dl class="grid gap-3 px-4 py-3 text-xs sm:grid-cols-3">
									<div class="min-w-0">
										<dt class="text-[10px] font-semibold text-slate-600">경매 구성</dt>
										<dd class="mt-1 truncate text-slate-400">
											10인 {season.auctionTenCount} · 20인 {season.auctionTwentyCount}
										</dd>
									</div>
									<div class="min-w-0">
										<dt class="text-[10px] font-semibold text-slate-600">최근 확정 경기</dt>
										<dd class="mt-1 truncate text-slate-400">
											{season.lastGameAt ? formatSeoulDateTime(season.lastGameAt) : '없음'}
										</dd>
									</div>
									<div class="min-w-0">
										<dt class="text-[10px] font-semibold text-slate-600">시작한 운영자</dt>
										<dd class="mt-1 truncate text-slate-400">{season.createdBy ?? '시스템'}</dd>
									</div>
								</dl>
							</div>
						{/if}
					</div>
				{:else}
					<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
						종료된 시즌이 없습니다.
					</div>
				{/each}
			</div>
		</section>
	</div>

	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			onOpenAutoFocus={focusSeasonName}
			onEscapeKeydown={preventCloseWhileSubmitting}
			onInteractOutside={preventCloseWhileSubmitting}
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="flex items-center gap-2 text-sm font-semibold text-slate-50">
					<CalendarRange size={16} class="text-cyan-200" aria-hidden="true" />
					새 시즌 시작
				</Dialog.Title>
				<Dialog.Close
					type="button"
					disabled={submitting}
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="새 시즌 시작 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>

			<form id="start-season-form" onsubmit={startSeason} aria-busy={submitting}>
				<div class="grid gap-4 p-4">
					<Dialog.Description
						class="flex gap-2 border border-amber-300/20 bg-amber-300/6 px-3 py-2.5 text-xs leading-5 text-slate-400"
					>
						<TriangleAlert size={15} class="mt-0.5 shrink-0 text-amber-200" aria-hidden="true" />
						<span>
							<strong class="block font-semibold text-amber-100">{currentSeason.name} 종료</strong>
							새 시즌이 즉시 시작되며 기존 전적과 MMR 기록은 그대로 보존됩니다.
						</span>
					</Dialog.Description>

					<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
						시즌 이름
						<input
							bind:this={seasonNameInput}
							bind:value={newSeasonName}
							disabled={submitting}
							required
							maxlength="100"
							placeholder="예: 2026 시즌 4"
							autocomplete="off"
							oninput={clearStartError}
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-400/45 disabled:opacity-55"
						/>
					</label>

					{#if startError}
						<p
							class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
							role="alert"
						>
							{startError}
						</p>
					{/if}
				</div>
				<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
					<Dialog.Close
						type="button"
						disabled={submitting}
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
					>
						취소
					</Dialog.Close>
					<button
						type="submit"
						disabled={!canStartSeason || submitting}
						class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
					>
						{submitting ? '시작 중' : '새 시즌 시작'}
					</button>
				</footer>
			</form>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
