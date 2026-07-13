<script lang="ts">
	import { ChevronDown, FileClock, Search, ShieldCheck } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import type { GuildAuditEntry } from '$lib/contracts/workflow';
	import { formatRelativeTime } from '$lib/workflow/presentation';

	type AuditCategory = '멤버' | 'MMR' | '내전' | '경매' | '설정' | '기타';
	type CategoryFilter = '전체' | AuditCategory;

	interface Props {
		entries: GuildAuditEntry[];
		nextCursor: string | null;
		loadingMore?: boolean;
		loadMoreError?: string | null;
		currentUserId?: string;
		onLoadMore?: () => void | Promise<void>;
	}

	interface ActionPresentation {
		label: string;
		category: AuditCategory;
	}

	interface PresentedEntry {
		raw: GuildAuditEntry;
		actionLabel: string;
		category: AuditCategory;
		targetLabel: string;
		principalLabel: string;
		principalKey: string;
		relativeTime: string;
		seoulTime: string;
		beforeSummary: string | null;
		afterSummary: string | null;
	}

	let {
		entries,
		nextCursor,
		loadingMore = false,
		loadMoreError = null,
		currentUserId,
		onLoadMore
	}: Props = $props();

	let query = $state('');
	let categoryFilter = $state<CategoryFilter>('전체');
	let operatorFilter = $state('전체');
	let expandedEntryId = $state<string | null>(null);

	const categories: CategoryFilter[] = ['전체', '멤버', 'MMR', '내전', '경매', '설정', '기타'];
	const actionPresentations: Record<string, ActionPresentation> = {
		GUILD_SETUP: { label: '길드 초기 설정', category: '설정' },
		GUILD_SETTINGS_UPDATED: { label: '길드 설정 변경', category: '설정' },
		SEASON_ROTATED: { label: '새 시즌 시작', category: '설정' },
		MEMBER_MMR_ADJUSTED: { label: 'MMR 수동 조정', category: 'MMR' },
		MEMBER_STATUS_CHANGED: { label: '멤버 상태 변경', category: '멤버' },
		MEMBER_STATUS_RECOVERED: { label: '멤버 상태 복구', category: '멤버' },
		RECRUITMENT_CREATE: { label: '모집 생성', category: '내전' },
		RECRUITMENT_BIND_MESSAGE: { label: '모집 메시지 연결', category: '내전' },
		RECRUITMENT_JOIN: { label: '모집 참여', category: '내전' },
		RECRUITMENT_LEAVE: { label: '모집 이탈', category: '내전' },
		RECRUITMENT_SET_PREFERRED_LANES: { label: '선호 라인 변경', category: '내전' },
		RECRUITMENT_CLOSE: { label: '모집 마감', category: '내전' },
		GAME_RESULT_RECORDED: { label: '게임 결과 입력', category: '내전' },
		AUCTION_BID_CONFIRMED: { label: '경매 낙찰 확정', category: '경매' },
		LINEUP_REVERTED: { label: '라인업 확정 복구', category: '경매' }
	};
	const targetTypeLabels: Record<string, string> = {
		GUILD: '길드',
		GUILD_CONFIG: '길드 설정',
		GUILD_MEMBER: '길드 멤버',
		GUILD_MEMBER_MMR: '멤버 MMR',
		SEASON: '시즌',
		RECRUITMENT: '모집',
		SERIES: '시리즈',
		GAME: '게임',
		AUCTION: '경매',
		AUCTION_TOURNAMENT: '경매 내전'
	};
	const seoulFormatter = new Intl.DateTimeFormat('ko-KR', {
		timeZone: 'Asia/Seoul',
		year: 'numeric',
		month: '2-digit',
		day: '2-digit',
		hour: '2-digit',
		minute: '2-digit',
		second: '2-digit',
		hour12: false
	});

	const presentedEntries = $derived(entries.map(presentEntry));
	const operators = $derived.by(() => {
		const unique: { value: string; label: string }[] = [];
		for (const entry of presentedEntries) {
			if (!unique.some((operator) => operator.value === entry.principalKey)) {
				unique.push({ value: entry.principalKey, label: entry.principalLabel });
			}
		}
		return unique;
	});
	const filteredEntries = $derived(
		presentedEntries.filter((entry) => {
			const normalizedQuery = query.trim().toLocaleLowerCase('ko-KR');
			const matchesQuery =
				!normalizedQuery ||
				entry.actionLabel.toLocaleLowerCase('ko-KR').includes(normalizedQuery) ||
				entry.raw.action.toLocaleLowerCase('ko-KR').includes(normalizedQuery) ||
				entry.targetLabel.toLocaleLowerCase('ko-KR').includes(normalizedQuery) ||
				entry.principalLabel.toLocaleLowerCase('ko-KR').includes(normalizedQuery) ||
				entry.raw.note?.toLocaleLowerCase('ko-KR').includes(normalizedQuery);
			return (
				matchesQuery &&
				(categoryFilter === '전체' || entry.category === categoryFilter) &&
				(operatorFilter === '전체' || entry.principalKey === operatorFilter)
			);
		})
	);

	function presentEntry(entry: GuildAuditEntry): PresentedEntry {
		const presentation = presentAction(entry.action);
		return {
			raw: entry,
			actionLabel: presentation.label,
			category: presentation.category,
			targetLabel: presentTarget(entry),
			principalLabel: presentPrincipal(entry),
			principalKey: `${entry.principalType}:${entry.principalId}`,
			relativeTime: formatRelativeTime(entry.createdAt),
			seoulTime: seoulFormatter.format(new Date(entry.createdAt)),
			beforeSummary: formatSummary(entry.beforeSummary),
			afterSummary: formatSummary(entry.afterSummary)
		};
	}

	function presentAction(action: string): ActionPresentation {
		return actionPresentations[action] ?? { label: action, category: '기타' };
	}

	function presentPrincipal(entry: GuildAuditEntry): string {
		let label: string;
		if (entry.principalDisplayName) label = entry.principalDisplayName;
		else if (entry.principalType === 'BOT') label = 'MonkeyBot';
		else if (entry.principalType === 'SYSTEM') label = '시스템';
		else if (entry.principalType === 'MIGRATION') label = '데이터 이전';
		else label = `사용자 ${compactIdentifier(entry.principalId)}`;
		return entry.principalType === 'USER' && entry.principalId === currentUserId
			? `${label} (나)`
			: label;
	}

	function presentTarget(entry: GuildAuditEntry): string {
		const typeLabel = entry.targetType
			? (targetTypeLabels[entry.targetType] ?? entry.targetType)
			: null;
		if (entry.targetDisplayName && typeLabel) return `${entry.targetDisplayName} · ${typeLabel}`;
		if (entry.targetDisplayName) return entry.targetDisplayName;
		if (entry.targetId && typeLabel) return `${typeLabel} · ${compactIdentifier(entry.targetId)}`;
		if (entry.targetId) return compactIdentifier(entry.targetId);
		return typeLabel ?? '대상 없음';
	}

	function compactIdentifier(value: string): string {
		return value.length > 16 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value;
	}

	function formatSummary(summary: GuildAuditEntry['beforeSummary']): string | null {
		if (summary === null) return null;
		try {
			return JSON.stringify(summary, null, 2) ?? String(summary);
		} catch {
			return '요약을 표시할 수 없습니다.';
		}
	}

	function toggleEntry(entryId: string) {
		expandedEntryId = expandedEntryId === entryId ? null : entryId;
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 items-center gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<FileClock size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
		<div class="min-w-0">
			<h2 class="truncate text-sm font-semibold text-slate-50">감사 로그</h2>
			<p class="text-xs text-slate-500">길드 관리 작업과 상태 변경 기록</p>
		</div>
	</header>

	<div
		class="grid gap-2 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-3 md:grid-cols-[minmax(0,1fr)_140px_140px]"
	>
		<label
			class="flex h-10 min-w-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3"
		>
			<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
			<input
				bind:value={query}
				placeholder="작업 또는 대상 검색"
				aria-label="감사 로그 검색"
				class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
			/>
		</label>
		<select
			bind:value={categoryFilter}
			aria-label="감사 로그 분류"
			class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
		>
			{#each categories as category (category)}
				<option value={category}>{category === '전체' ? '전체 작업' : category}</option>
			{/each}
		</select>
		<select
			bind:value={operatorFilter}
			aria-label="감사 로그 실행자"
			class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
		>
			<option value="전체">전체 실행자</option>
			{#each operators as operator (operator.value)}
				<option value={operator.value}>{operator.label}</option>
			{/each}
		</select>
	</div>

	<div class="overflow-x-auto">
		<div class="min-w-[850px]">
			<div
				class="grid grid-cols-[88px_158px_minmax(208px,1fr)_112px_88px_44px] gap-x-3 border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>분류</span><span>작업</span><span>대상</span><span>실행자</span><span>시각</span><span
				></span>
			</div>
			<div class="divide-y divide-[var(--color-app-border-muted)]">
				{#each filteredEntries as entry (entry.raw.id)}
					<div>
						<button
							type="button"
							onclick={() => toggleEntry(entry.raw.id)}
							aria-expanded={expandedEntryId === entry.raw.id}
							class="grid w-full grid-cols-[88px_158px_minmax(208px,1fr)_112px_88px_44px] items-center gap-x-3 px-4 py-3 text-left hover:bg-[var(--color-app-hover)]"
						>
							<div class="min-w-0 justify-self-start">
								<StatusPill label={entry.category} tone="neutral" />
							</div>
							<div class="flex min-w-0 items-center gap-2">
								<ShieldCheck size={14} class="shrink-0 text-cyan-200" aria-hidden="true" />
								<strong class="truncate text-xs text-slate-200">{entry.actionLabel}</strong>
							</div>
							<span class="truncate text-xs text-slate-400">{entry.targetLabel}</span>
							<span class="truncate text-xs text-slate-500">{entry.principalLabel}</span>
							<time
								datetime={entry.raw.createdAt}
								title={`${entry.seoulTime} (KST)`}
								class="text-xs text-slate-600">{entry.relativeTime}</time
							>
							<ChevronDown
								size={15}
								class={`justify-self-end text-slate-500 transition ${expandedEntryId === entry.raw.id ? 'rotate-180' : ''}`}
								aria-hidden="true"
							/>
						</button>
						{#if expandedEntryId === entry.raw.id}
							<div
								class="grid gap-3 border-t border-[var(--color-app-border-muted)] bg-slate-950/20 px-4 py-3 sm:grid-cols-[100px_minmax(0,1fr)]"
							>
								<div class="justify-self-start">
									<StatusPill
										label={entry.raw.result === 'REVERTED' ? '복구됨' : '완료'}
										tone={entry.raw.result === 'REVERTED' ? 'warning' : 'success'}
									/>
								</div>
								<div class="min-w-0 space-y-3">
									{#if entry.raw.note}
										<p class="text-xs leading-5 text-slate-300">{entry.raw.note}</p>
									{/if}
									{#if entry.beforeSummary || entry.afterSummary}
										<div class="grid gap-3 sm:grid-cols-2">
											{#if entry.beforeSummary}
												<div class="min-w-0 border-l-2 border-slate-700 pl-3">
													<span class="text-[10px] font-semibold text-slate-500">변경 전</span>
													<pre
														class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all font-mono text-[10px] leading-4 text-slate-400">{entry.beforeSummary}</pre>
												</div>
											{/if}
											{#if entry.afterSummary}
												<div class="min-w-0 border-l-2 border-cyan-900 pl-3">
													<span class="text-[10px] font-semibold text-slate-500">변경 후</span>
													<pre
														class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all font-mono text-[10px] leading-4 text-slate-400">{entry.afterSummary}</pre>
												</div>
											{/if}
										</div>
									{/if}
									<div class="flex flex-wrap gap-x-4 gap-y-1 font-mono text-[10px] text-slate-600">
										<span>작업 {entry.raw.id}</span>
										<span>{entry.seoulTime} KST</span>
										{#if entry.raw.checkpointId}<span>체크포인트 {entry.raw.checkpointId}</span
											>{/if}
										{#if entry.raw.requestId}<span>요청 {entry.raw.requestId}</span>{/if}
									</div>
								</div>
							</div>
						{/if}
					</div>
				{:else}
					<div class="px-4 py-10 text-center text-sm text-slate-600">
						{entries.length === 0
							? '아직 기록된 감사 로그가 없습니다.'
							: '조건에 맞는 로그가 없습니다.'}
					</div>
				{/each}
			</div>
		</div>
	</div>

	{#if (nextCursor && onLoadMore) || loadMoreError}
		<footer
			class="flex flex-col items-center gap-2 border-t border-[var(--color-app-border-muted)] px-4 py-3"
		>
			{#if loadMoreError}
				<p class="text-xs text-rose-300" role="alert">{loadMoreError}</p>
			{/if}
			{#if nextCursor && onLoadMore}
				<button
					type="button"
					onclick={() => onLoadMore?.()}
					disabled={loadingMore}
					aria-busy={loadingMore}
					class="h-9 min-w-28 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-4 text-xs font-semibold text-slate-300 hover:border-cyan-800 hover:text-cyan-100 disabled:cursor-wait disabled:opacity-60"
				>
					{loadingMore ? '불러오는 중...' : '더 보기'}
				</button>
			{/if}
		</footer>
	{/if}
</section>
