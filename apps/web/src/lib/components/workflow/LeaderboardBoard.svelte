<script lang="ts">
	import { ChevronLeft, ChevronRight, Search, Trophy, UserRound } from 'lucide-svelte';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import PositionIcon from '$lib/components/workflow/PositionIcon.svelte';
	import type {
		GuildLeaderboardPlayer,
		GuildLeaderboardRoleStat,
		GuildLeaderboardSnapshot
	} from '$lib/contracts/workflow';
	import { findChampion, type ChampionCatalog } from '$lib/datadragon';
	import type { Lane } from '$lib/fixtures/workflow';

	type LeaderboardRole = 'COMPOSITE' | Lane;

	interface DisplayRow {
		rank: number;
		player: GuildLeaderboardPlayer;
		mmr: number;
		games: number;
		wins: number;
		losses: number;
		championId: string | null;
		rolesPlayed: number;
	}

	interface Props {
		catalog: ChampionCatalog;
		snapshot: GuildLeaderboardSnapshot | null;
		loadError?: string | null;
		onOpenPlayer?: (playerId: string) => void | Promise<void>;
	}

	const roleOrder: LeaderboardRole[] = ['COMPOSITE', 'TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const roleLabel: Record<LeaderboardRole, string> = {
		COMPOSITE: '통합',
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '원딜',
		SUPPORT: '서폿'
	};

	let { catalog, snapshot, loadError = null, onOpenPlayer }: Props = $props();
	let selectedRole = $state<LeaderboardRole>('COMPOSITE');
	let query = $state('');
	let currentPage = $state(1);
	let selectedPlayerId = $state<string | null>(null);
	const pageSize = 25;

	const players = $derived(snapshot?.players ?? []);
	const rows = $derived(buildRows(selectedRole));
	const filteredRows = $derived(
		rows
			.filter((row) => {
				const needle = query.trim().toLocaleLowerCase();
				return (
					!needle ||
					row.player.name.toLocaleLowerCase().includes(needle) ||
					(row.player.tag?.toLocaleLowerCase().includes(needle) ?? false)
				);
			})
	);
	const pageCount = $derived(Math.max(1, Math.ceil(filteredRows.length / pageSize)));
	const activePage = $derived(Math.min(currentPage, pageCount));
	const visibleRows = $derived(
		filteredRows.slice((activePage - 1) * pageSize, activePage * pageSize)
	);
	const visibleStart = $derived(visibleRows[0]?.rank ?? 0);
	const visibleEnd = $derived(visibleRows.at(-1)?.rank ?? 0);
	const seasonLabel = $derived(
		loadError ? '조회 실패' : (snapshot?.season?.name ?? '활성 시즌 없음')
	);
	const emptyMessage = $derived.by(() => {
		if (loadError) return loadError;
		if (snapshot?.season === null) return '현재 진행 중인 시즌이 없습니다.';
		if (players.length === 0) return '이번 시즌에 집계된 내전 기록이 없습니다.';
		if (query.trim()) return '검색 조건에 맞는 플레이어가 없습니다.';
		return '선택한 라인에 집계된 기록이 없습니다.';
	});

	function buildRows(role: LeaderboardRole): DisplayRow[] {
		return players
			.flatMap((player) => {
				const roleStats = Object.values(player.stats) as GuildLeaderboardRoleStat[];
				if (role === 'COMPOSITE') {
					const mainStat = player.stats[player.mainRole];
					if (!mainStat) return [];
					const games = roleStats.reduce((sum, stat) => sum + stat.games, 0);
					const wins = roleStats.reduce((sum, stat) => sum + stat.wins, 0);
					return [
						{
							rank: 0,
							player,
							mmr: player.compositeMmr,
							games,
							wins,
							losses: games - wins,
							championId: mainStat.championId,
							rolesPlayed: roleStats.length
						}
					];
				}

				const stat = player.stats[role];
				if (!stat) return [];
				return [
					{
						rank: 0,
						player,
						mmr: stat.mmr,
						games: stat.games,
						wins: stat.wins,
						losses: stat.games - stat.wins,
						championId: stat.championId,
						rolesPlayed: roleStats.length
					}
				];
			})
			.sort((a, b) => b.mmr - a.mmr || a.player.id.localeCompare(b.player.id))
			.map((row, index) => ({ ...row, rank: index + 1 }));
	}

	function selectRole(role: LeaderboardRole) {
		selectedRole = role;
		query = '';
		currentPage = 1;
		selectedPlayerId = null;
	}

	function resetPagination() {
		currentPage = 1;
		selectedPlayerId = null;
	}

	function setPage(nextPage: number) {
		currentPage = Math.max(1, Math.min(nextPage, pageCount));
		selectedPlayerId = null;
	}

	function winRate(row: DisplayRow): number {
		return row.games === 0 ? 0 : Math.round((row.wins / row.games) * 100);
	}

	function profileIconUrl(id: number): string {
		return `/dd/cdn/${catalog.version}/img/profileicon/${id}.png`;
	}

	function rankClass(rank: number): string {
		if (rank === 1) return 'text-amber-300';
		if (rank === 2) return 'text-slate-300';
		if (rank === 3) return 'text-orange-300';
		return 'text-slate-500';
	}

	function openPlayerRecords(playerId: string) {
		if (!onOpenPlayer) return;
		void onOpenPlayer(playerId);
	}
</script>

<section
	class="flex h-full min-h-0 flex-1 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 shrink-0 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<Trophy size={20} class="text-amber-300" aria-hidden="true" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">리더보드</h2>
				<p class="text-xs text-slate-500">{seasonLabel} · {rows.length}명</p>
			</div>
		</div>

		<div class="flex min-w-0 flex-wrap items-center gap-2">
			<label
				class="flex h-9 w-64 max-w-full items-center gap-2 rounded-sm border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 focus-within:border-cyan-400/50"
			>
				<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
				<input
					bind:value={query}
					oninput={resetPagination}
					type="search"
					class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
					placeholder="플레이어 검색"
					aria-label="플레이어 검색"
				/>
			</label>
		</div>
	</header>

	<div
		class="flex min-w-0 shrink-0 overflow-x-auto border-b border-[var(--color-app-border)] px-3 pt-2"
	>
		{#each roleOrder as role (role)}
			<button
				type="button"
				class={`flex h-9 min-w-20 items-center justify-center gap-1.5 border-b-2 px-3 text-xs font-semibold ${
					selectedRole === role
						? 'border-cyan-300 text-cyan-100'
						: 'border-transparent text-slate-500 hover:text-slate-300'
				}`}
				onclick={() => selectRole(role)}
			>
				{#if role === 'COMPOSITE'}
					<Trophy size={14} aria-hidden="true" />
				{:else}
					<PositionIcon {role} size="sm" />
				{/if}
				{roleLabel[role]}
			</button>
		{/each}
	</div>

	<div class="min-h-0 flex-1 overflow-auto overscroll-contain">
		<div class="min-w-[940px]">
			<div
				class="sticky top-0 z-10 grid grid-cols-[72px_minmax(240px,1fr)_110px_120px_100px_200px_100px] items-center border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-3 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>순위</span><span>플레이어</span><span class="text-right">MMR</span><span
					class="text-right">전적</span
				><span class="text-right">승률</span><span class="pl-6">주 챔피언</span><span
					class="text-right">{selectedRole === 'COMPOSITE' ? '라인' : '게임'}</span
				>
			</div>

			{#each visibleRows as row (row.player.id)}
				{@const champion = row.championId ? findChampion(catalog, row.championId) : undefined}
				<button
					type="button"
					class={`grid min-h-[62px] w-full grid-cols-[72px_minmax(240px,1fr)_110px_120px_100px_200px_100px] items-center border-b border-[var(--color-app-border-muted)] px-3 text-left hover:bg-[var(--color-app-hover)] ${
						row.player.isMe ? 'bg-cyan-400/[0.055] shadow-[inset_3px_0_0_#22d3ee]' : ''
					} ${selectedPlayerId === row.player.id ? 'outline outline-1 -outline-offset-1 outline-cyan-300/40' : ''}`}
					aria-label={`${row.player.name} 선택. 두 번 클릭하면 내전 전적 열기`}
					aria-pressed={selectedPlayerId === row.player.id}
					onclick={() => (selectedPlayerId = row.player.id)}
					ondblclick={() => openPlayerRecords(row.player.id)}
				>
					<strong class={`text-sm tabular-nums ${rankClass(row.rank)}`}>#{row.rank}</strong>
					<span class="flex min-w-0 items-center gap-3">
						<span
							class="relative grid size-10 shrink-0 place-items-center overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-500"
							aria-hidden="true"
						>
							<UserRound size={20} />
							{#if row.player.profileIconId !== null}
								<img
									src={profileIconUrl(row.player.profileIconId)}
									alt=""
									class="absolute inset-0 size-full object-cover"
									loading="lazy"
									onerror={(event) => event.currentTarget.setAttribute('hidden', '')}
								/>
							{/if}
						</span>
						<span class="min-w-0">
							<strong class="block truncate text-sm text-slate-100">{row.player.name}</strong>
							<small class="block truncate text-[10px] text-slate-600"
								>{row.player.tag ?? 'Riot ID 비공개'}</small
							>
						</span>
					</span>
					<strong class="text-right text-sm tabular-nums text-cyan-200"
						>{Math.round(row.mmr)}</strong
					>
					<span class="text-right text-xs tabular-nums text-slate-400"
						>{row.wins}승 {row.losses}패</span
					>
					<span class="text-right text-xs font-semibold tabular-nums text-slate-300"
						>{winRate(row)}%</span
					>
					<span class="min-w-0 pl-6">
						{#if champion}
							<ChampionFace {champion} version={catalog.version} size="sm" showName />
						{:else}
							<span class="text-xs text-slate-600">{row.championId ?? '기록 없음'}</span>
						{/if}
					</span>
					<span class="text-right text-xs tabular-nums text-slate-500">
						{selectedRole === 'COMPOSITE' ? `${row.rolesPlayed}라인` : `${row.games}게임`}
					</span>
				</button>
			{:else}
				<p class="px-4 py-12 text-center text-sm text-slate-500">{emptyMessage}</p>
			{/each}
		</div>
	</div>

	<footer
		class="flex min-h-12 shrink-0 items-center justify-between gap-3 border-t border-[var(--color-app-border)] px-4 py-2"
	>
		<p class="text-xs tabular-nums text-slate-500">
			{#if filteredRows.length > 0}
				{visibleStart}-{visibleEnd} / {filteredRows.length}명
			{:else}
				0명
			{/if}
		</p>
		<div class="flex items-center gap-1.5">
			<button
				type="button"
				class="grid size-8 place-items-center border border-[var(--color-app-border)] text-slate-400 hover:border-cyan-400/35 hover:text-cyan-100 disabled:cursor-not-allowed disabled:opacity-35"
				aria-label="이전 페이지"
				title="이전 페이지"
				disabled={activePage <= 1}
				onclick={() => setPage(activePage - 1)}
			>
				<ChevronLeft size={15} aria-hidden="true" />
			</button>
			<span class="min-w-12 text-center text-xs font-semibold tabular-nums text-slate-300">
				{activePage} / {pageCount}
			</span>
			<button
				type="button"
				class="grid size-8 place-items-center border border-[var(--color-app-border)] text-slate-400 hover:border-cyan-400/35 hover:text-cyan-100 disabled:cursor-not-allowed disabled:opacity-35"
				aria-label="다음 페이지"
				title="다음 페이지"
				disabled={activePage >= pageCount}
				onclick={() => setPage(activePage + 1)}
			>
				<ChevronRight size={15} aria-hidden="true" />
			</button>
		</div>
	</footer>
</section>
