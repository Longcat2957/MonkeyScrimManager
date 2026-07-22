<script lang="ts">
	import { goto } from '$app/navigation';
	import { Dialog } from 'bits-ui';
	import {
		Archive,
		ArrowRight,
		Gavel,
		LoaderCircle,
		Plus,
		Swords,
		Trophy,
		UsersRound,
		X
	} from 'lucide-svelte';
	import {
		auctionRecruitmentErrorText,
		createAuctionRecruitment
	} from '$lib/api/auctionRecruitment';
	import {
		auctionListErrorText,
		fetchActionableAuctionRecruitments,
		fetchAuctionTournaments
	} from '$lib/api/auctionList';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import {
		auctionBestOfValues,
		type AuctionBestOf,
		type AuctionRecruitmentDto,
		type AuctionTournamentDto
	} from '$lib/contracts/workflow';
	import { buildAuctionView } from '$lib/workflow/presentation';
	import { SvelteSet } from 'svelte/reactivity';
	import { formatAuctionRules } from '$lib/workflow/auctionRules';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	type AuctionPageSource = {
		actionableRecruitments: {
			recruitments: AuctionRecruitmentDto[];
			nextCursor: string | null;
		};
		activeTournaments: {
			tournaments: AuctionTournamentDto[];
			nextCursor: string | null;
		};
		completedTournaments: {
			tournaments: AuctionTournamentDto[];
			nextCursor: string | null;
		};
	};
	type AuctionPaginationState = {
		source: AuctionPageSource;
		recruitments: AuctionRecruitmentDto[];
		activeTournaments: AuctionTournamentDto[];
		completedTournaments: AuctionTournamentDto[];
		nextRecruitmentCursor: string | null;
		nextActiveTournamentCursor: string | null;
		nextCompletedTournamentCursor: string | null;
		activeLoading: boolean;
		completedLoading: boolean;
		activeError: string | null;
		completedError: string | null;
	};
	type PageResult<T> = { page: T; error: null } | { page: null; error: unknown };

	let auctionPagination = $state<AuctionPaginationState | null>(null);
	const currentAuctionPagination = $derived(
		auctionPagination?.source === data.auctionPages ? auctionPagination : null
	);
	const actionableRecruitments = $derived(
		currentAuctionPagination?.recruitments ?? data.auctionPages.actionableRecruitments.recruitments
	);
	const activeTournaments = $derived(
		currentAuctionPagination?.activeTournaments ?? data.auctionPages.activeTournaments.tournaments
	);
	const completedTournaments = $derived(
		currentAuctionPagination?.completedTournaments ??
			data.auctionPages.completedTournaments.tournaments
	);
	const nextRecruitmentCursor = $derived(
		currentAuctionPagination?.nextRecruitmentCursor ??
			data.auctionPages.actionableRecruitments.nextCursor
	);
	const nextActiveTournamentCursor = $derived(
		currentAuctionPagination?.nextActiveTournamentCursor ??
			data.auctionPages.activeTournaments.nextCursor
	);
	const nextCompletedTournamentCursor = $derived(
		currentAuctionPagination?.nextCompletedTournamentCursor ??
			data.auctionPages.completedTournaments.nextCursor
	);
	const activeAuctionLoading = $derived(currentAuctionPagination?.activeLoading ?? false);
	const completedAuctionLoading = $derived(currentAuctionPagination?.completedLoading ?? false);
	const activeAuctionLoadMoreError = $derived(currentAuctionPagination?.activeError ?? null);
	const completedAuctionLoadMoreError = $derived(currentAuctionPagination?.completedError ?? null);
	const hasMoreActiveAuctionWork = $derived(
		nextRecruitmentCursor !== null || nextActiveTournamentCursor !== null
	);
	const hasMoreCompletedAuctionWork = $derived(nextCompletedTournamentCursor !== null);
	const auctionView = $derived(
		buildAuctionView({
			guildId: data.guildId,
			recruitments: actionableRecruitments,
			activeTournaments,
			completedTournaments
		})
	);
	const activeAuctionWork = $derived(auctionView.activeWork);
	const completedAuctionWork = $derived(auctionView.completedWork);
	const iconByKind = {
		recruitment: UsersRound,
		bidding: Gavel,
		bracket: Swords,
		completed: Trophy,
		archived: Archive
	} as const;

	type AuctionWorkItem = (typeof activeAuctionWork)[number];
	type AuctionFormatOption = {
		format: AuctionRecruitmentDto['format'];
		label: string;
		detail: string;
	};

	const auctionFormatOptions: AuctionFormatOption[] = [
		{ format: 'TEN_PERSON', label: '10인 경매', detail: '10명 · 2팀' },
		{ format: 'TWENTY_PERSON', label: '20인 경매', detail: '20명 · 4팀' }
	];

	let createDialogOpen = $derived(Boolean(data.openCreateDialog));
	let creatingRecruitment = $state(false);
	let createError = $state('');
	let createIdempotencyKey = $state<string | null>(null);
	let createFormat = $state<AuctionRecruitmentDto['format']>('TEN_PERSON');
	let createSemifinalBestOf = $state<AuctionBestOf>(3);
	let createFinalBestOf = $state<AuctionBestOf>(3);
	let createHardFearless = $state(false);
	const consumedRecruitmentCursors = new SvelteSet<string>();
	const consumedActiveTournamentCursors = new SvelteSet<string>();
	const consumedCompletedTournamentCursors = new SvelteSet<string>();
	let consumedCursorSource: AuctionPageSource | null = null;

	function currentPaginationState(source: AuctionPageSource): AuctionPaginationState {
		const current = auctionPagination?.source === source ? auctionPagination : null;
		return {
			source,
			recruitments: current?.recruitments ?? source.actionableRecruitments.recruitments,
			activeTournaments: current?.activeTournaments ?? source.activeTournaments.tournaments,
			completedTournaments:
				current?.completedTournaments ?? source.completedTournaments.tournaments,
			nextRecruitmentCursor:
				current?.nextRecruitmentCursor ?? source.actionableRecruitments.nextCursor,
			nextActiveTournamentCursor:
				current?.nextActiveTournamentCursor ?? source.activeTournaments.nextCursor,
			nextCompletedTournamentCursor:
				current?.nextCompletedTournamentCursor ?? source.completedTournaments.nextCursor,
			activeLoading: current?.activeLoading ?? false,
			completedLoading: current?.completedLoading ?? false,
			activeError: current?.activeError ?? null,
			completedError: current?.completedError ?? null
		};
	}

	function resetConsumedCursors(source: AuctionPageSource) {
		if (consumedCursorSource === source) return;
		consumedRecruitmentCursors.clear();
		consumedActiveTournamentCursors.clear();
		consumedCompletedTournamentCursors.clear();
		consumedCursorSource = source;
	}

	function mergePages<T extends { id: string }>(existing: T[], additions: T[]): T[] {
		const knownIds = new Set(existing.map((item) => item.id));
		return [...existing, ...additions.filter((item) => !knownIds.has(item.id))];
	}

	async function capturePage<T>(request: Promise<T>): Promise<PageResult<T>> {
		try {
			return { page: await request, error: null };
		} catch (error) {
			return { page: null, error };
		}
	}

	function repeatedCursor(
		nextCursor: string | null,
		cursor: string,
		consumedCursors: SvelteSet<string>
	): boolean {
		return nextCursor === cursor || consumedCursors.has(nextCursor ?? '');
	}

	async function loadMoreActiveAuctionWork() {
		const source = data.auctionPages;
		resetConsumedCursors(source);
		const state = currentPaginationState(source);
		const recruitmentCursor = state.nextRecruitmentCursor;
		const tournamentCursor = state.nextActiveTournamentCursor;
		if (
			state.activeLoading ||
			state.completedLoading ||
			(!recruitmentCursor && !tournamentCursor) ||
			(recruitmentCursor && consumedRecruitmentCursors.has(recruitmentCursor)) ||
			(tournamentCursor && consumedActiveTournamentCursors.has(tournamentCursor))
		) {
			return;
		}

		const sourceGuildId = data.guildId;
		if (recruitmentCursor) consumedRecruitmentCursors.add(recruitmentCursor);
		if (tournamentCursor) consumedActiveTournamentCursors.add(tournamentCursor);
		auctionPagination = { ...state, activeLoading: true, activeError: null };
		const [recruitmentResult, tournamentResult] = await Promise.all([
			recruitmentCursor
				? capturePage(
						fetchActionableAuctionRecruitments(globalThis.fetch, sourceGuildId, {
							limit: 100,
							cursor: recruitmentCursor
						})
					)
				: Promise.resolve(null),
			tournamentCursor
				? capturePage(
						fetchAuctionTournaments(globalThis.fetch, sourceGuildId, 'active', {
							limit: 100,
							cursor: tournamentCursor
						})
					)
				: Promise.resolve(null)
		]);
		if (data.auctionPages !== source || data.guildId !== sourceGuildId) return;

		let recruitments = state.recruitments;
		let activeTournaments = state.activeTournaments;
		let nextRecruitmentCursor = recruitmentCursor;
		let nextActiveTournamentCursor = tournamentCursor;
		const errors: string[] = [];
		if (recruitmentCursor && recruitmentResult) {
			if (recruitmentResult.page) {
				recruitments = mergePages(recruitments, recruitmentResult.page.recruitments);
				if (
					repeatedCursor(
						recruitmentResult.page.nextCursor,
						recruitmentCursor,
						consumedRecruitmentCursors
					)
				) {
					nextRecruitmentCursor = null;
					errors.push('경매 모집 목록 페이지가 갱신되지 않았습니다. 페이지를 새로고침해 주세요.');
				} else {
					nextRecruitmentCursor = recruitmentResult.page.nextCursor;
				}
			} else {
				consumedRecruitmentCursors.delete(recruitmentCursor);
				errors.push(auctionListErrorText(recruitmentResult.error));
			}
		}
		if (tournamentCursor && tournamentResult) {
			if (tournamentResult.page) {
				activeTournaments = mergePages(activeTournaments, tournamentResult.page.tournaments);
				if (
					repeatedCursor(
						tournamentResult.page.nextCursor,
						tournamentCursor,
						consumedActiveTournamentCursors
					)
				) {
					nextActiveTournamentCursor = null;
					errors.push('진행 중인 경매 페이지가 갱신되지 않았습니다. 페이지를 새로고침해 주세요.');
				} else {
					nextActiveTournamentCursor = tournamentResult.page.nextCursor;
				}
			} else {
				consumedActiveTournamentCursors.delete(tournamentCursor);
				errors.push(auctionListErrorText(tournamentResult.error));
			}
		}
		auctionPagination = {
			...state,
			recruitments,
			activeTournaments,
			nextRecruitmentCursor,
			nextActiveTournamentCursor,
			activeLoading: false,
			activeError: errors.join(' ') || null
		};
	}

	async function loadMoreCompletedAuctionWork() {
		const source = data.auctionPages;
		resetConsumedCursors(source);
		const state = currentPaginationState(source);
		const cursor = state.nextCompletedTournamentCursor;
		if (
			!cursor ||
			state.activeLoading ||
			state.completedLoading ||
			consumedCompletedTournamentCursors.has(cursor)
		) {
			return;
		}

		const sourceGuildId = data.guildId;
		consumedCompletedTournamentCursors.add(cursor);
		auctionPagination = { ...state, completedLoading: true, completedError: null };
		const result = await capturePage(
			fetchAuctionTournaments(globalThis.fetch, sourceGuildId, 'completed', { limit: 100, cursor })
		);
		if (data.auctionPages !== source || data.guildId !== sourceGuildId) return;

		if (!result.page) {
			consumedCompletedTournamentCursors.delete(cursor);
			auctionPagination = {
				...state,
				completedLoading: false,
				completedError: auctionListErrorText(result.error)
			};
			return;
		}

		const nextCursor = repeatedCursor(
			result.page.nextCursor,
			cursor,
			consumedCompletedTournamentCursors
		)
			? null
			: result.page.nextCursor;
		auctionPagination = {
			...state,
			completedTournaments: mergePages(state.completedTournaments, result.page.tournaments),
			nextCompletedTournamentCursor: nextCursor,
			completedLoading: false,
			completedError:
				nextCursor === null && result.page.nextCursor !== null
					? '완료된 경매 목록 페이지가 갱신되지 않았습니다. 페이지를 새로고침해 주세요.'
					: null
		};
	}

	function openCreateDialog() {
		if (creatingRecruitment) return;
		createError = '';
		createIdempotencyKey = null;
		createFormat = 'TEN_PERSON';
		createSemifinalBestOf = 3;
		createFinalBestOf = 3;
		createHardFearless = false;
		createDialogOpen = true;
	}

	function selectCreateFormat(format: AuctionRecruitmentDto['format']) {
		if (creatingRecruitment || createFormat === format) return;
		createFormat = format;
		createIdempotencyKey = null;
	}

	function selectCreateBestOf(stage: 'semifinal' | 'final', bestOf: AuctionBestOf) {
		if (creatingRecruitment) return;
		if (stage === 'semifinal') {
			if (createSemifinalBestOf === bestOf) return;
			createSemifinalBestOf = bestOf;
		} else {
			if (createFinalBestOf === bestOf) return;
			createFinalBestOf = bestOf;
		}
		createIdempotencyKey = null;
	}

	function toggleCreateHardFearless() {
		if (creatingRecruitment) return;
		createHardFearless = !createHardFearless;
		createIdempotencyKey = null;
	}

	async function createNewAuctionRecruitment() {
		if (creatingRecruitment) return;
		const idempotencyKey = (createIdempotencyKey ??= globalThis.crypto.randomUUID());
		creatingRecruitment = true;
		createError = '';
		try {
			const created = await createAuctionRecruitment(
				data.guildId,
				{
					format: createFormat,
					semifinalBestOf: createFormat === 'TWENTY_PERSON' ? createSemifinalBestOf : null,
					finalBestOf: createFinalBestOf,
					hardFearless: createHardFearless
				},
				idempotencyKey
			);
			createIdempotencyKey = null;
			createDialogOpen = false;
			await goto(created.recruitment.href);
		} catch (cause) {
			createError = auctionRecruitmentErrorText(cause);
		} finally {
			creatingRecruitment = false;
		}
	}

	function displayNumber(item: AuctionWorkItem): string {
		return item.kind === '모집' ? `#${item.displayNumber}` : `A-${item.displayNumber}`;
	}
</script>

{#snippet AuctionRow(item: AuctionWorkItem)}
	{@const Icon = iconByKind[item.iconKind]}
	<div
		class="grid gap-3 px-4 py-3 md:grid-cols-[minmax(220px,1fr)_100px_minmax(180px,1fr)_minmax(116px,15rem)] md:items-center"
	>
		<div class="flex min-w-0 items-center gap-3">
			<span
				class={`grid size-9 shrink-0 place-items-center border ${
					item.iconKind === 'archived'
						? 'border-[var(--color-app-border)] bg-[var(--color-app-surface-muted)] text-slate-400'
						: 'border-cyan-400/25 bg-cyan-400/8 text-cyan-200'
				}`}
			>
				<Icon size={16} aria-hidden="true" />
			</span>
			<div class="min-w-0">
				<strong class="block truncate text-sm text-slate-100">{item.title}</strong>
				<small class="text-[10px] text-slate-600">{displayNumber(item)}</small>
			</div>
		</div>
		<span class="text-xs text-slate-500">{item.kind}</span>
		<div class="flex min-w-0 items-center gap-2">
			<StatusPill label={item.status} tone={item.tone} />
			<span class="truncate text-xs text-slate-500">{item.detail}</span>
		</div>
		<div class="flex flex-wrap items-center gap-2 md:justify-end">
			<a
				href={item.href}
				class="inline-flex h-9 items-center justify-center gap-1.5 border border-cyan-400/30 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10"
			>
				{item.actionLabel}
				<ArrowRight size={14} aria-hidden="true" />
			</a>
		</div>
	</div>
{/snippet}

<svelte:head>
	<title>경매 내전 | MonkeyBot</title>
</svelte:head>

<div class="space-y-5">
	<WorkspaceHeader kicker="경매 내전" title="경매 작업" description="모집부터 완료 경매까지">
		{#snippet actions()}
			<button
				type="button"
				disabled={creatingRecruitment}
				onclick={openCreateDialog}
				class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35"
			>
				{#if creatingRecruitment}
					<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
				{:else}
					<Plus size={14} aria-hidden="true" />
				{/if}
				{creatingRecruitment ? '생성 중' : '경매 모집'}
			</button>
		{/snippet}
	</WorkspaceHeader>

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
			class="hidden grid-cols-[minmax(220px,1fr)_100px_minmax(180px,1fr)_minmax(116px,15rem)] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
		>
			<span>경매</span><span>구분</span><span>현재 상태</span><span></span>
		</div>
		<div class="divide-y divide-[var(--color-app-border-muted)]">
			{#each activeAuctionWork as item (item.id)}
				{@render AuctionRow(item)}
			{:else}
				<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
					진행 중인 경매 작업이 없습니다.
				</div>
			{/each}
		</div>
		{#if hasMoreActiveAuctionWork || activeAuctionLoadMoreError}
			<div class="grid gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				{#if hasMoreActiveAuctionWork}
					<button
						type="button"
						disabled={activeAuctionLoading || completedAuctionLoading}
						onclick={() => void loadMoreActiveAuctionWork()}
						class="inline-flex h-9 w-full items-center justify-center gap-1.5 border border-[var(--color-app-border)] text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] disabled:opacity-35"
					>
						{#if activeAuctionLoading}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
							불러오는 중
						{:else}
							더 불러오기
						{/if}
					</button>
				{/if}
				{#if activeAuctionLoadMoreError}
					<p class="text-xs leading-5 text-rose-300" role="alert">{activeAuctionLoadMoreError}</p>
				{/if}
			</div>
		{/if}
	</section>

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-2">
				<Archive size={16} class="shrink-0 text-slate-400" aria-hidden="true" />
				<h2 class="truncate text-sm font-semibold text-slate-100">완료 및 보관</h2>
			</div>
			<span class="text-xs font-semibold text-slate-500">{completedAuctionWork.length}건</span>
		</header>
		<div
			class="hidden grid-cols-[minmax(220px,1fr)_100px_minmax(180px,1fr)_minmax(116px,15rem)] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
		>
			<span>경매</span><span>구분</span><span>결과 · 보관</span><span></span>
		</div>
		<div class="divide-y divide-[var(--color-app-border-muted)]">
			{#each completedAuctionWork as item (item.id)}
				{@render AuctionRow(item)}
			{:else}
				<div class="grid min-h-28 place-items-center px-4 py-8 text-sm text-slate-600">
					완료된 경매가 없습니다.
				</div>
			{/each}
		</div>
		{#if hasMoreCompletedAuctionWork || completedAuctionLoadMoreError}
			<div class="grid gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				{#if hasMoreCompletedAuctionWork}
					<button
						type="button"
						disabled={activeAuctionLoading || completedAuctionLoading}
						onclick={() => void loadMoreCompletedAuctionWork()}
						class="inline-flex h-9 w-full items-center justify-center gap-1.5 border border-[var(--color-app-border)] text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] disabled:opacity-35"
					>
						{#if completedAuctionLoading}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
							불러오는 중
						{:else}
							더 불러오기
						{/if}
					</button>
				{/if}
				{#if completedAuctionLoadMoreError}
					<p class="text-xs leading-5 text-rose-300" role="alert">
						{completedAuctionLoadMoreError}
					</p>
				{/if}
			</div>
		{/if}
	</section>
</div>

<Dialog.Root bind:open={createDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">경매 모집</Dialog.Title>
				<Dialog.Close
					disabled={creatingRecruitment}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="경매 모집 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>

			<div class="grid gap-4 p-4">
				<Dialog.Description class="text-xs leading-5 text-slate-400">
					참가와 취소는 Discord 모집 메시지에서 진행합니다. 운영자는 생성 후 모집 화면에서 길드
					멤버를 직접 추가하거나 제거할 수 있습니다.
				</Dialog.Description>

				<div class="grid grid-cols-2 gap-2" aria-label="경매 인원 선택">
					{#each auctionFormatOptions as option (option.format)}
						<button
							type="button"
							aria-pressed={createFormat === option.format}
							disabled={creatingRecruitment}
							onclick={() => selectCreateFormat(option.format)}
							class={`grid min-h-20 content-center gap-1 border px-3 text-left transition-colors disabled:opacity-35 ${
								createFormat === option.format
									? 'border-cyan-300/55 bg-cyan-400/10 text-cyan-50'
									: 'border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-300 hover:border-cyan-400/30'
							}`}
						>
							<strong class="text-sm font-semibold">{option.label}</strong>
							<span class="text-xs text-slate-500">{option.detail}</span>
						</button>
					{/each}
				</div>

				{#if createFormat === 'TWENTY_PERSON'}
					<div class="grid gap-2">
						<span class="text-[11px] font-semibold text-slate-500">4강 형식</span>
						<div
							class="grid grid-cols-3 border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
							role="radiogroup"
							aria-label="4강 시리즈 형식"
						>
							{#each auctionBestOfValues as bestOf (bestOf)}
								<button
									type="button"
									role="radio"
									aria-checked={createSemifinalBestOf === bestOf}
									disabled={creatingRecruitment}
									onclick={() => selectCreateBestOf('semifinal', bestOf)}
									class={`h-9 text-xs font-semibold disabled:opacity-35 ${
										createSemifinalBestOf === bestOf
											? 'bg-cyan-400/15 text-cyan-100'
											: 'text-slate-500 hover:text-slate-200'
									}`}
								>
									BO{bestOf}
								</button>
							{/each}
						</div>
					</div>
				{/if}

				<div class="grid gap-2">
					<span class="text-[11px] font-semibold text-slate-500"
						>{createFormat === 'TEN_PERSON' ? '시리즈 형식' : '결승 형식'}</span
					>
					<div
						class="grid grid-cols-3 border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-1"
						role="radiogroup"
						aria-label={createFormat === 'TEN_PERSON' ? '시리즈 형식' : '결승 시리즈 형식'}
					>
						{#each auctionBestOfValues as bestOf (bestOf)}
							<button
								type="button"
								role="radio"
								aria-checked={createFinalBestOf === bestOf}
								disabled={creatingRecruitment}
								onclick={() => selectCreateBestOf('final', bestOf)}
								class={`h-9 text-xs font-semibold disabled:opacity-35 ${
									createFinalBestOf === bestOf
										? 'bg-cyan-400/15 text-cyan-100'
										: 'text-slate-500 hover:text-slate-200'
								}`}
							>
								BO{bestOf}
							</button>
						{/each}
					</div>
				</div>

				<button
					type="button"
					role="switch"
					aria-checked={createHardFearless}
					disabled={creatingRecruitment}
					onclick={toggleCreateHardFearless}
					class="flex min-h-12 items-center justify-between gap-4 border border-[var(--color-app-border)] px-3 text-left enabled:hover:bg-[var(--color-app-hover)] disabled:opacity-35"
				>
					<span class="grid gap-0.5">
						<strong class="text-xs font-semibold text-slate-200">하드 피어리스</strong>
						<span class="text-[11px] text-slate-500"
							>각 경기에서 이전 세트의 픽은 다시 선택할 수 없습니다.</span
						>
					</span>
					<span
						class={`relative h-5 w-9 shrink-0 border transition-colors ${
							createHardFearless
								? 'border-cyan-300/65 bg-cyan-400/35'
								: 'border-slate-600 bg-slate-800'
						}`}
						aria-hidden="true"
					>
						<span
							class={`absolute top-0.5 size-3.5 bg-slate-100 transition-transform ${
								createHardFearless ? 'translate-x-4' : 'translate-x-0.5'
							}`}
						></span>
					</span>
				</button>

				<p class="text-xs font-semibold text-slate-400">
					{formatAuctionRules(createFormat, {
						hardFearless: createHardFearless,
						semifinalBestOf: createFormat === 'TWENTY_PERSON' ? createSemifinalBestOf : null,
						finalBestOf: createFinalBestOf
					})}
				</p>

				{#if createError}
					<p
						class="border border-rose-400/20 bg-rose-400/5 px-3 py-2 text-xs leading-5 text-rose-300"
						role="alert"
					>
						{createError}
					</p>
				{/if}
			</div>

			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					disabled={creatingRecruitment}
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={creatingRecruitment}
					onclick={createNewAuctionRecruitment}
					class="inline-flex h-9 items-center gap-1.5 border border-cyan-300/35 bg-cyan-400/15 px-3 text-xs font-semibold text-cyan-50 disabled:opacity-35"
				>
					{#if creatingRecruitment}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 생성 중
					{:else}
						<Plus size={14} aria-hidden="true" /> 모집 만들기
					{/if}
				</button>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
