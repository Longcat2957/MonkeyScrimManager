<script lang="ts">
	import {
		ArrowDown,
		ArrowUp,
		ChevronsUpDown,
		ChevronDown,
		ChevronUp,
		RefreshCw
	} from 'lucide-svelte';
	import {
		fetchMoreRiotMatches,
		refreshRiotRecords,
		riotRecordsErrorText,
		RiotRecordsApiError
	} from '$lib/api/riotRecords';
	import { championIconUrl, profileIconUrl, type ChampionCatalog } from '$lib/datadragon';
	import type {
		RiotChampionAggregate,
		RiotMatchParticipant,
		RiotRecentMatch,
		RiotRecords
	} from '$lib/contracts/riotRecords';

	interface Lookup {
		gameName: string;
		tagLine: string;
		platform: string;
	}

	interface Props {
		initialRecords: RiotRecords;
		lookup: Lookup;
		catalog: ChampionCatalog | null;
	}

	type QueueFilter = 'ALL' | 'RANKED' | 'NORMAL';
	type ChampionSortKey =
		| 'championName'
		| 'games'
		| 'winRate'
		| 'kda'
		| 'kills'
		| 'deaths'
		| 'assists'
		| 'creepScore'
		| 'goldEarned'
		| 'wins';

	let { initialRecords, lookup, catalog }: Props = $props();
	let refreshedRecords = $state<RiotRecords | null>(null);
	const records = $derived(refreshedRecords ?? initialRecords);
	let queueFilter = $state<QueueFilter>('ALL');
	let championSortKey = $state<ChampionSortKey>('games');
	let championSortDirection = $state<'asc' | 'desc'>('desc');
	let championsExpanded = $state(false);
	let expandedMatchId = $state<string | null>(null);
	let refreshing = $state(false);
	let refreshCooldown = $state(0);
	let refreshNotice = $state<string | null>(null);
	let refreshError = $state<string | null>(null);
	let loadingMoreMatches = $state(false);
	let matchesHaveMoreOverride = $state<boolean | null>(null);
	let nextMatchOffset = $state(0);
	let loadMoreError = $state<string | null>(null);

	$effect(() => {
		const sourceRecords = initialRecords;
		refreshedRecords = null;
		championsExpanded = false;
		expandedMatchId = null;
		refreshNotice = null;
		refreshError = null;
		resetMatchPagination(sourceRecords);
	});

	$effect(() => {
		const refreshedAt = records.refreshedAt;
		const updateCooldown = () => {
			refreshCooldown = secondsUntilRefresh(refreshedAt);
		};
		updateCooldown();
		const timer = globalThis.setInterval(updateCooldown, 1_000);
		return () => globalThis.clearInterval(timer);
	});

	const matchesWithTarget = $derived.by(() =>
		records.recentMatches
			.map((match) => ({ match, player: targetPlayer(match) }))
			.filter(
				(entry): entry is { match: RiotRecentMatch; player: RiotMatchParticipant } =>
					entry.player !== null
			)
	);
	const filteredMatches = $derived.by(() =>
		matchesWithTarget.filter(({ match }) => {
			if (queueFilter === 'RANKED') return isRankedQueue(match.queueId);
			if (queueFilter === 'NORMAL') return !isRankedQueue(match.queueId);
			return true;
		})
	);
	const sampleWins = $derived(matchesWithTarget.filter(({ player }) => player.win).length);
	const sampleGames = $derived(matchesWithTarget.length);
	const sampleKda = $derived.by(() => {
		const totals = matchesWithTarget.reduce(
			(acc, { player }) => ({
				kills: acc.kills + player.kills,
				deaths: acc.deaths + player.deaths,
				assists: acc.assists + player.assists
			}),
			{ kills: 0, deaths: 0, assists: 0 }
		);
		return kdaRatio(totals.kills, totals.deaths, totals.assists);
	});
	const averageCs = $derived.by(() => {
		if (sampleGames === 0) return '0';
		const total = matchesWithTarget.reduce((sum, { player }) => sum + player.creepScore, 0);
		return (total / sampleGames).toFixed(1);
	});
	const sortedChampionAggregates = $derived.by(() =>
		[...records.championAggregates].sort((left, right) => {
			const leftValue = championSortValue(left, championSortKey);
			const rightValue = championSortValue(right, championSortKey);
			const comparison =
				typeof leftValue === 'string' && typeof rightValue === 'string'
					? leftValue.localeCompare(rightValue, 'ko-KR')
					: Number(leftValue) - Number(rightValue);
			return (
				(championSortDirection === 'asc' ? comparison : -comparison) ||
				left.championName.localeCompare(right.championName)
			);
		})
	);
	const visibleChampionAggregates = $derived(
		championsExpanded ? sortedChampionAggregates : sortedChampionAggregates.slice(0, 5)
	);
	const matchesHaveMore = $derived(matchesHaveMoreOverride ?? snapshotHasMore(records));

	const queueFilters: { value: QueueFilter; label: string }[] = [
		{ value: 'ALL', label: '전체' },
		{ value: 'RANKED', label: '랭크' },
		{ value: 'NORMAL', label: '일반' }
	];

	const spellImages: Record<number, string> = {
		1: 'SummonerBoost.png',
		3: 'SummonerExhaust.png',
		4: 'SummonerFlash.png',
		6: 'SummonerHaste.png',
		7: 'SummonerHeal.png',
		11: 'SummonerSmite.png',
		12: 'SummonerTeleport.png',
		13: 'SummonerMana.png',
		14: 'SummonerDot.png',
		21: 'SummonerBarrier.png',
		32: 'SummonerSnowball.png'
	};

	async function refresh() {
		if (refreshing || refreshCooldown > 0) return;
		refreshing = true;
		refreshNotice = null;
		refreshError = null;
		try {
			refreshedRecords = await refreshRiotRecords(lookup);
			championsExpanded = false;
			resetMatchPagination(refreshedRecords);
			refreshNotice =
				refreshedRecords.cache.state === 'stale'
					? 'Riot 응답 지연으로 기존 기록을 표시합니다.'
					: '전적을 갱신했습니다.';
		} catch (error) {
			refreshError = riotRecordsErrorText(error);
		} finally {
			refreshing = false;
		}
	}

	async function loadMoreMatches() {
		if (loadingMoreMatches || !matchesHaveMore) return;
		loadingMoreMatches = true;
		loadMoreError = null;
		try {
			const nextPage = await fetchMoreRiotMatches(lookup, nextMatchOffset, 10);
			if (nextPage.targetPuuid !== records.account.puuid) {
				throw new RiotRecordsApiError(
					'조회한 계정과 추가 전적의 계정이 일치하지 않습니다.',
					409,
					'record_account_mismatch'
				);
			}
			const knownMatchIds = new Set(records.recentMatches.map((match) => match.matchId));
			const recentMatches = [
				...records.recentMatches,
				...nextPage.matches.filter((match) => !knownMatchIds.has(match.matchId))
			];
			refreshedRecords = {
				...records,
				recentMatches,
				sample: {
					...records.sample,
					recentMatches: {
						requested: Math.max(records.sample.recentMatches.requested, recentMatches.length),
						returned: recentMatches.length
					}
				}
			};
			matchesHaveMoreOverride = nextPage.page.hasMore;
			nextMatchOffset = nextPage.page.nextOffset ?? recentMatches.length;
		} catch (error) {
			loadMoreError = riotRecordsErrorText(error);
		} finally {
			loadingMoreMatches = false;
		}
	}

	function resetMatchPagination(snapshot: RiotRecords) {
		nextMatchOffset = snapshot.recentMatches.length;
		matchesHaveMoreOverride = null;
		loadMoreError = null;
	}

	function snapshotHasMore(snapshot: RiotRecords): boolean {
		return (
			snapshot.sample.recentMatches.returned === snapshot.sample.recentMatches.requested &&
			snapshot.sample.recentMatches.returned > 0
		);
	}

	function targetPlayer(match: RiotRecentMatch): RiotMatchParticipant | null {
		return (
			match.participants.find((participant) => participant.puuid === match.targetPuuid) ?? null
		);
	}

	function championName(id: string): string {
		return catalog?.byId[id]?.name ?? id;
	}

	function championImage(id: string): string {
		return championIconUrl(catalog?.byId[id]?.imageFull ?? `${id}.png`, dataDragonVersion());
	}

	function dataDragonVersion(): string {
		if (catalog) return catalog.version;
		const gameVersion = records.recentMatches[0]?.gameVersion.split('.').slice(0, 2).join('.');
		return gameVersion ? `${gameVersion}.1` : '16.13.1';
	}

	function spellImage(id: number): string | null {
		const image = spellImages[id];
		return image ? `/dd/cdn/${dataDragonVersion()}/img/spell/${image}` : null;
	}

	function itemImage(id: number): string {
		return `/dd/cdn/${dataDragonVersion()}/img/item/${id}.png`;
	}

	function rankQueueLabel(queueType: string): string {
		if (queueType === 'RANKED_SOLO_5x5') return '솔로랭크';
		if (queueType === 'RANKED_FLEX_SR') return '자유랭크';
		return queueType.replaceAll('_', ' ');
	}

	function rankName(tier: string | null, rank: string | null): string {
		return tier ? `${tier} ${rank ?? ''}`.trim() : 'UNRANKED';
	}

	function rankIcon(tier: string | null): string | null {
		return tier ? `/riot/ranks/${tier.toLowerCase()}.png` : null;
	}

	function queueLabel(queueId: number): string {
		const labels: Record<number, string> = {
			400: '일반 드래프트',
			420: '솔로랭크',
			430: '일반',
			440: '자유랭크',
			450: '무작위 총력전',
			490: '빠른 대전',
			1700: '아레나'
		};
		return labels[queueId] ?? `QUEUE ${queueId}`;
	}

	function isRankedQueue(queueId: number): boolean {
		return queueId === 420 || queueId === 440;
	}

	function formatDuration(seconds: number): string {
		const minutes = Math.floor(seconds / 60);
		return `${minutes}:${String(seconds % 60).padStart(2, '0')}`;
	}

	function formatRelativeTime(value: string): string {
		const timestamp = new Date(value).getTime();
		if (!Number.isFinite(timestamp)) return value;
		const seconds = Math.round((timestamp - Date.now()) / 1000);
		const formatter = new Intl.RelativeTimeFormat('ko', { numeric: 'auto' });
		if (Math.abs(seconds) < 3600) return formatter.format(Math.round(seconds / 60), 'minute');
		if (Math.abs(seconds) < 86400) return formatter.format(Math.round(seconds / 3600), 'hour');
		if (Math.abs(seconds) < 2592000) return formatter.format(Math.round(seconds / 86400), 'day');
		return formatter.format(Math.round(seconds / 2592000), 'month');
	}

	function formatDateTime(value: string): string {
		const timestamp = new Date(value);
		return Number.isNaN(timestamp.getTime())
			? value
			: timestamp.toLocaleString('ko-KR', { dateStyle: 'medium', timeStyle: 'short' });
	}

	function formatSeasonDate(unixSeconds: number): string {
		return new Date(unixSeconds * 1_000).toLocaleDateString('ko-KR', {
			year: 'numeric',
			month: 'long',
			day: 'numeric'
		});
	}

	function formatNumber(value: number): string {
		return new Intl.NumberFormat('ko-KR').format(value);
	}

	function secondsUntilRefresh(refreshedAt: string): number {
		const timestamp = new Date(refreshedAt).getTime();
		if (!Number.isFinite(timestamp)) return 0;
		return Math.max(0, Math.ceil((timestamp + 30_000 - Date.now()) / 1_000));
	}

	function average(value: number, games: number): string {
		return games > 0 ? (value / games).toFixed(1) : '0.0';
	}

	function championSortValue(
		champion: RiotChampionAggregate,
		key: ChampionSortKey
	): string | number {
		if (key === 'championName') return championName(champion.championName);
		if (key === 'winRate') return champion.wins / champion.games;
		if (key === 'kda') {
			return champion.deaths === 0
				? Number.POSITIVE_INFINITY
				: (champion.kills + champion.assists) / champion.deaths;
		}
		if (key === 'wins') return champion.wins;
		return champion[key] / (key === 'games' ? 1 : champion.games);
	}

	function setChampionSort(key: ChampionSortKey) {
		if (championSortKey === key) {
			championSortDirection = championSortDirection === 'desc' ? 'asc' : 'desc';
			return;
		}
		championSortKey = key;
		championSortDirection = key === 'championName' ? 'asc' : 'desc';
	}

	function winRate(wins: number, games: number): string {
		return games > 0 ? `${((wins / games) * 100).toFixed(1)}%` : '0.0%';
	}

	function kdaRatio(kills: number, deaths: number, assists: number): string {
		return deaths === 0 ? 'Perfect' : ((kills + assists) / deaths).toFixed(2);
	}

	function positionLabel(position: string): string {
		const labels: Record<string, string> = {
			TOP: '탑',
			JUNGLE: '정글',
			MIDDLE: '미드',
			MID: '미드',
			BOTTOM: '바텀',
			UTILITY: '서폿',
			SUPPORT: '서폿'
		};
		return labels[position] ?? '-';
	}

	function teamPlayers(match: RiotRecentMatch, teamId: number): RiotMatchParticipant[] {
		const order = ['TOP', 'JUNGLE', 'MIDDLE', 'MID', 'BOTTOM', 'UTILITY', 'SUPPORT', ''];
		return match.participants
			.filter((participant) => participant.teamId === teamId)
			.toSorted((a, b) => order.indexOf(a.teamPosition) - order.indexOf(b.teamPosition));
	}

	function teamTotals(match: RiotRecentMatch, teamId: number) {
		return teamPlayers(match, teamId).reduce(
			(totals, player) => ({
				kills: totals.kills + player.kills,
				deaths: totals.deaths + player.deaths,
				assists: totals.assists + player.assists,
				gold: totals.gold + player.goldEarned
			}),
			{ kills: 0, deaths: 0, assists: 0, gold: 0 }
		);
	}

	function teamWon(match: RiotRecentMatch, teamId: number): boolean {
		return teamPlayers(match, teamId)[0]?.win ?? false;
	}

	function killParticipation(match: RiotRecentMatch, player: RiotMatchParticipant): string {
		const teamKills = teamTotals(match, player.teamId).kills;
		return teamKills > 0
			? `${Math.round(((player.kills + player.assists) / teamKills) * 100)}%`
			: '0%';
	}

	function damageWidth(match: RiotRecentMatch, value: number, field: 'dealt' | 'taken'): number {
		const maximum = Math.max(
			1,
			...match.participants.map((participant) =>
				field === 'dealt' ? participant.damageDealtToChampions : participant.damageTaken
			)
		);
		return Math.max(4, Math.round((value / maximum) * 100));
	}
</script>

<div class="records-page">
	<div class="records-shell">
		<section class="profile-layout">
			<div class="profile-avatar">
				{#if records.profileIconId !== null}
					<img
						src={profileIconUrl(records.profileIconId, dataDragonVersion())}
						alt={`${records.account.gameName}#${records.account.tagLine} 프로필`}
						decoding="async"
					/>
				{:else}
					<span>{records.account.gameName.slice(0, 1)}</span>
				{/if}
			</div>
			<div class="profile-copy">
				<p class="profile-meta-line">
					{records.platform} · LEVEL {records.summonerLevel ?? '-'}
				</p>
				<h1 class="profile-name-line">
					{records.account.gameName}<span>#{records.account.tagLine}</span>
				</h1>
				<div class="profile-actions">
					<button
						type="button"
						class="profile-action-line"
						disabled={refreshing || refreshCooldown > 0}
						title={refreshCooldown > 0
							? `${refreshCooldown}초 후 다시 갱신할 수 있습니다.`
							: undefined}
						onclick={refresh}
					>
						<RefreshCw
							size={14}
							class={refreshing ? 'animate-spin' : undefined}
							aria-hidden="true"
						/>
						{refreshing
							? '갱신 중'
							: refreshCooldown > 0
								? `${refreshCooldown}초 후 갱신`
								: '전적 갱신'}
					</button>
					{#if refreshNotice}<span class="refresh-notice" aria-live="polite">{refreshNotice}</span
						>{/if}
					{#if refreshError}<span class="refresh-error" role="alert">{refreshError}</span>{/if}
				</div>
			</div>
			<div class="record-meta">
				<span>최근 {records.recentMatches.length}경기</span>
				<span>시즌 랭크 {records.sample.championAggregates.analyzedMatches}경기</span>
				<span class:stale={records.cache.state === 'stale'}>
					{records.cache.state === 'stale' ? '캐시 데이터' : '최신 데이터'}
				</span>
				<time datetime={records.refreshedAt}>{formatDateTime(records.refreshedAt)}</time>
			</div>
		</section>

		<section class="rank-grid" aria-label="현재 랭크">
			{#each records.ranks as rank (rank.queueType)}
				<div class="rank-panel">
					<div class="rank-mark">
						{#if rankIcon(rank.tier)}
							<img
								src={rankIcon(rank.tier) ?? ''}
								alt={rank.tier ?? ''}
								width="82"
								height="82"
								decoding="async"
							/>
						{:else}
							<span>-</span>
						{/if}
					</div>
					<div class="rank-copy">
						<span>{rankQueueLabel(rank.queueType)}</span>
						<strong>{rankName(rank.tier, rank.rank)}</strong>
						<span>{rank.leaguePoints} LP · {rank.wins}승 {rank.losses}패</span>
					</div>
				</div>
			{:else}
				<div class="rank-panel empty-rank">
					<div class="rank-mark"><span>-</span></div>
					<div class="rank-copy">
						<span>랭크</span><strong>UNRANKED</strong><span>최근 기록 없음</span>
					</div>
				</div>
			{/each}
		</section>

		<section class="table-section" aria-labelledby="champion-stats-title">
			<header class="section-heading">
				<div>
					<h2 id="champion-stats-title">이번 시즌 챔피언</h2>
					<p>
						{formatSeasonDate(records.sample.championAggregates.seasonStartUnixSeconds)} 이후 랭크
						{records.sample.championAggregates.analyzedMatches}경기 기준{records.sample
							.championAggregates.truncated
							? ' · 최대 탐색 범위까지 집계'
							: ''}
					</p>
				</div>
			</header>
			<div class="table-scroll">
				<div class="data-table">
					<div class="data-row data-head">
						<span>{@render SortHeader('챔피언', 'championName')}</span>
						<span>{@render SortHeader('게임', 'games')}</span>
						<span>{@render SortHeader('승률', 'winRate')}</span>
						<span>{@render SortHeader('KDA', 'kda')}</span>
						<span>{@render SortHeader('킬', 'kills')}</span>
						<span>{@render SortHeader('데스', 'deaths')}</span>
						<span>{@render SortHeader('어시', 'assists')}</span>
						<span>{@render SortHeader('CS', 'creepScore')}</span>
						<span>{@render SortHeader('골드', 'goldEarned')}</span>
						<span>{@render SortHeader('승 / 패', 'wins')}</span>
					</div>
					{#each visibleChampionAggregates as champion (champion.championName)}
						<div class="data-row">
							<span>
								<img
									src={championImage(champion.championName)}
									alt=""
									class="table-champion-icon"
									loading="lazy"
									decoding="async"
								/>
								{championName(champion.championName)}
							</span>
							<span>{champion.games}</span>
							<span>{winRate(champion.wins, champion.games)}</span>
							<span>{kdaRatio(champion.kills, champion.deaths, champion.assists)}</span>
							<span>{average(champion.kills, champion.games)}</span>
							<span>{average(champion.deaths, champion.games)}</span>
							<span>{average(champion.assists, champion.games)}</span>
							<span>{average(champion.creepScore, champion.games)}</span>
							<span
								>{formatNumber(Math.round(champion.goldEarned / Math.max(1, champion.games)))}</span
							>
							<span>{champion.wins} / {champion.games - champion.wins}</span>
						</div>
					{:else}
						<p class="empty-table">이번 시즌에 집계할 경기가 없습니다.</p>
					{/each}
				</div>
			</div>
			{#if sortedChampionAggregates.length > 5}
				<button
					type="button"
					class="champion-more"
					aria-expanded={championsExpanded}
					onclick={() => (championsExpanded = !championsExpanded)}
				>
					{championsExpanded
						? '챔피언 목록 접기'
						: `전체 ${sortedChampionAggregates.length}개 보기`}
					{#if championsExpanded}
						<ChevronUp size={15} aria-hidden="true" />
					{:else}
						<ChevronDown size={15} aria-hidden="true" />
					{/if}
				</button>
			{/if}
		</section>

		<section class="match-layout" aria-labelledby="recent-matches-title">
			<header class="match-heading">
				<div>
					<h2 id="recent-matches-title">최근 경기</h2>
					<p>경기를 펼치면 양 팀 10명의 상세 기록을 확인할 수 있습니다.</p>
				</div>
				<div class="match-filters" aria-label="경기 종류">
					{#each queueFilters as filter (filter.value)}
						<button
							type="button"
							class:active={queueFilter === filter.value}
							onclick={() => (queueFilter = filter.value)}
						>
							{filter.label}
						</button>
					{/each}
				</div>
			</header>

			<div class="summary-grid">
				<div class="summary-block">
					<strong>{sampleGames ? `${((sampleWins / sampleGames) * 100).toFixed(1)}%` : '0%'}</strong
					>
					<span>{sampleWins}승 {sampleGames - sampleWins}패</span>
				</div>
				<div class="summary-block"><strong>{sampleKda}</strong><span>평균 KDA</span></div>
				<div class="summary-block"><strong>{averageCs}</strong><span>평균 CS</span></div>
			</div>

			<div class="match-list-scroll">
				<div class="match-list">
					{#each filteredMatches as entry (entry.match.matchId)}
						<div class="match-entry">
							<div class:loss-row={!entry.player.win} class="match-row">
								<div class="match-champion">
									<img
										src={championImage(entry.player.championName)}
										alt={championName(entry.player.championName)}
										loading="lazy"
										decoding="async"
									/>
									<span>{entry.player.championLevel}</span>
								</div>
								<span>
									<b>{entry.player.win ? '승리' : '패배'}</b>
									<small>{queueLabel(entry.match.queueId)}</small>
								</span>
								<span>
									<b
										>{kdaRatio(entry.player.kills, entry.player.deaths, entry.player.assists)} KDA</b
									>
									<small
										>{entry.player.kills} / {entry.player.deaths} / {entry.player.assists}</small
									>
								</span>
								<span>
									<span class="spell-icons">
										{#each entry.player.summonerSpellIds as spellId (spellId)}
											{#if spellImage(spellId)}<img
													src={spellImage(spellId) ?? ''}
													alt=""
													loading="lazy"
													decoding="async"
												/>{/if}
										{/each}
									</span>
									<small>{positionLabel(entry.player.teamPosition)}</small>
								</span>
								<span class="item-icons">
									{#each entry.player.itemIds.filter((id) => id > 0) as itemId, index (`${itemId}-${index}`)}
										<img src={itemImage(itemId)} alt="" loading="lazy" decoding="async" />
									{/each}
								</span>
								<span>
									<b>CS {entry.player.creepScore} · {formatNumber(entry.player.goldEarned)} G</b>
									<small
										>{formatDuration(entry.match.durationSeconds)} · {formatRelativeTime(
											entry.match.startedAt
										)}</small
									>
								</span>
								<button
									type="button"
									class="match-toggle"
									aria-label={`${championName(entry.player.championName)} 경기 상세`}
									aria-expanded={expandedMatchId === entry.match.matchId}
									onclick={() =>
										(expandedMatchId =
											expandedMatchId === entry.match.matchId ? null : entry.match.matchId)}
								>
									{#if expandedMatchId === entry.match.matchId}
										<ChevronUp size={16} aria-hidden="true" />
									{:else}
										<ChevronDown size={16} aria-hidden="true" />
									{/if}
								</button>
							</div>

							{#if expandedMatchId === entry.match.matchId}
								<section class="expanded-match" aria-label={`${entry.match.matchId} 경기 상세`}>
									{@render TeamSection(entry.match, 100, 'blue')}
									{@render TeamSection(entry.match, 200, 'red')}
								</section>
							{/if}
						</div>
					{:else}
						<p class="empty-matches">선택한 종류의 경기가 없습니다.</p>
					{/each}
				</div>
			</div>
			{#if matchesHaveMore}
				<button
					type="button"
					class="match-more"
					disabled={loadingMoreMatches}
					onclick={loadMoreMatches}
				>
					{#if loadingMoreMatches}
						<RefreshCw size={14} class="animate-spin" aria-hidden="true" />
						불러오는 중
					{:else}
						<ChevronDown size={15} aria-hidden="true" />
						최근 경기 더 보기
					{/if}
				</button>
			{/if}
			{#if loadMoreError}<p class="load-more-error" role="alert">{loadMoreError}</p>{/if}
		</section>
	</div>
</div>

{#snippet TeamSection(match: RiotRecentMatch, teamId: number, side: 'blue' | 'red')}
	{@const totals = teamTotals(match, teamId)}
	<div class={`team-summary ${side}-summary`}>
		<strong>{teamWon(match, teamId) ? '승리팀' : '패배팀'}</strong>
		<span>{totals.kills} / {totals.deaths} / {totals.assists}</span>
		<span>팀 골드 {formatNumber(totals.gold)}</span>
	</div>
	{#each teamPlayers(match, teamId) as player (player.puuid)}
		<div
			class:expanded-blue={side === 'blue'}
			class:expanded-red={side === 'red'}
			class="expanded-player-row"
		>
			<strong class="expanded-position">{positionLabel(player.teamPosition)}</strong>
			<div class="expanded-identity">
				<img
					src={championImage(player.championName)}
					alt={championName(player.championName)}
					loading="lazy"
					decoding="async"
				/>
				<p>
					<strong>{player.gameName ?? '이름 비공개'}</strong>
					<span>{player.tagLine ? `#${player.tagLine}` : championName(player.championName)}</span>
				</p>
			</div>
			<div class="expanded-spells">
				{#each player.summonerSpellIds as spellId (spellId)}
					{#if spellImage(spellId)}<img
							src={spellImage(spellId) ?? ''}
							alt=""
							loading="lazy"
							decoding="async"
						/>{/if}
				{/each}
			</div>
			<strong class="expanded-level">{player.championLevel}</strong>
			<div class="expanded-kda">
				<strong>{kdaRatio(player.kills, player.deaths, player.assists)}</strong>
				<span>{player.kills} / {player.deaths} / {player.assists}</span>
			</div>
			<span class="expanded-kill-rate">{killParticipation(match, player)}</span>
			<div class="expanded-items">
				{#each player.itemIds.filter((id) => id > 0) as itemId, index (`${itemId}-${index}`)}
					<img src={itemImage(itemId)} alt="" loading="lazy" decoding="async" />
				{/each}
			</div>
			<div class="expanded-farm">
				<strong>CS {player.creepScore}</strong><span>{formatNumber(player.goldEarned)} G</span>
			</div>
			<div class="expanded-meter">
				<strong>{formatNumber(player.damageDealtToChampions)}</strong>
				<span
					><i style={`width: ${damageWidth(match, player.damageDealtToChampions, 'dealt')}%`}
					></i></span
				>
			</div>
			<div class="expanded-meter taken">
				<strong>{formatNumber(player.damageTaken)}</strong>
				<span><i style={`width: ${damageWidth(match, player.damageTaken, 'taken')}%`}></i></span>
			</div>
			<div class="expanded-vision"><strong>{player.visionScore}</strong><span>시야</span></div>
		</div>
	{/each}
{/snippet}

{#snippet SortHeader(label: string, key: ChampionSortKey)}
	<button
		type="button"
		class:active-sort={championSortKey === key}
		class="sort-header"
		aria-label={`${label} 기준 ${championSortKey === key && championSortDirection === 'desc' ? '오름차순' : '내림차순'} 정렬`}
		onclick={() => setChampionSort(key)}
	>
		{label}
		{#if championSortKey === key}
			{#if championSortDirection === 'desc'}
				<ArrowDown size={12} aria-hidden="true" />
			{:else}
				<ArrowUp size={12} aria-hidden="true" />
			{/if}
		{:else}
			<ChevronsUpDown size={12} aria-hidden="true" />
		{/if}
	</button>
{/snippet}

<style>
	.records-page {
		padding: 12px 0 56px;
		background: var(--color-app-bg);
	}

	.records-shell {
		width: min(100%, 1280px);
		margin: 0 auto;
		padding: 12px;
		background: var(--color-app-bg-soft);
	}

	.profile-layout {
		display: grid;
		grid-template-columns: 112px minmax(0, 1fr);
		grid-template-rows: auto auto;
		gap: 12px 16px;
		min-height: 184px;
		padding: 12px;
		background: var(--color-app-surface);
	}

	.profile-avatar {
		display: grid;
		place-items: center;
		width: 112px;
		aspect-ratio: 1;
		overflow: hidden;
		background: var(--color-app-surface-muted);
		color: #67e8f9;
		font-size: 28px;
		font-weight: 800;
	}

	.profile-avatar img {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: cover;
	}

	.profile-copy {
		display: grid;
		align-content: center;
		justify-items: start;
		gap: 10px;
	}

	.profile-meta-line,
	.profile-name-line,
	.section-heading h2,
	.section-heading p,
	.match-heading h2,
	.match-heading p {
		margin: 0;
	}

	.profile-meta-line {
		color: #94a3b8;
		font-size: 12px;
		font-weight: 700;
	}

	.profile-name-line {
		color: #f8fafc;
		font-size: 28px;
		line-height: 1.1;
	}

	.profile-name-line span {
		margin-left: 2px;
		color: #64748b;
		font-weight: 600;
	}

	.profile-actions,
	.record-meta {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}

	.profile-action-line {
		display: inline-flex;
		height: 32px;
		align-items: center;
		gap: 6px;
		padding: 0 12px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		color: #cbd5e1;
		font-size: 12px;
		font-weight: 700;
		cursor: pointer;
	}

	.profile-action-line:hover:not(:disabled) {
		border-color: rgb(34 211 238 / 0.45);
		color: #cffafe;
	}

	.profile-action-line:disabled {
		cursor: wait;
		opacity: 0.6;
	}

	.refresh-notice,
	.refresh-error {
		font-size: 11px;
		font-weight: 700;
	}

	.refresh-notice {
		color: #6ee7b7;
	}

	.refresh-error {
		color: #fda4af;
	}

	.record-meta {
		grid-column: 1 / -1;
	}

	.record-meta span,
	.record-meta time {
		padding: 4px 8px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-control);
		color: #94a3b8;
		font-size: 10px;
		font-weight: 700;
	}

	.record-meta .stale {
		border-color: rgb(251 191 36 / 0.35);
		color: #fde68a;
	}

	.rank-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
		gap: 12px;
		margin-top: 12px;
	}

	.rank-panel {
		display: grid;
		grid-template-columns: 116px minmax(0, 1fr);
		align-items: center;
		min-height: 122px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}

	.rank-mark {
		display: grid;
		place-items: center;
		width: 82px;
		aspect-ratio: 1;
		margin: auto;
		background: var(--color-app-surface-muted);
		color: #64748b;
		font-size: 22px;
		font-weight: 800;
	}

	.rank-mark img {
		display: block;
		width: 82px;
		height: 82px;
		object-fit: contain;
	}

	.rank-copy {
		display: grid;
		gap: 8px;
		padding-right: 16px;
	}

	.rank-copy span:first-child {
		color: #94a3b8;
		font-size: 11px;
		font-weight: 700;
	}

	.rank-copy strong {
		color: #f8fafc;
		font-size: 16px;
	}

	.rank-copy span:last-child {
		color: #64748b;
		font-size: 11px;
	}

	.table-section,
	.match-layout {
		margin-top: 16px;
	}

	.section-heading,
	.match-heading {
		display: flex;
		min-height: 58px;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		border-bottom: 1px solid var(--color-app-border);
	}

	.section-heading h2,
	.match-heading h2 {
		color: #f8fafc;
		font-size: 14px;
	}

	.section-heading p,
	.match-heading p {
		margin-top: 4px;
		color: #64748b;
		font-size: 11px;
	}

	.table-scroll,
	.match-list-scroll {
		overflow-x: auto;
	}

	.data-table {
		min-width: 900px;
		border-left: 1px solid var(--color-app-border);
	}

	.data-row {
		display: grid;
		grid-template-columns: 170px repeat(8, minmax(70px, 1fr)) 90px;
		min-height: 42px;
	}

	.data-row > span {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 6px;
		border-right: 1px solid var(--color-app-border-muted);
		border-bottom: 1px solid var(--color-app-border-muted);
		background: #111a26;
		color: #94a3b8;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}

	.data-row > span:first-child {
		justify-content: flex-start;
		gap: 8px;
		padding-left: 10px;
		color: #e2e8f0;
		font-weight: 700;
	}

	.data-head > span {
		background: #1a2b35;
		color: #cbd5e1;
		font-size: 11px;
		font-weight: 700;
	}

	.sort-header {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 3px;
		width: 100%;
		height: 100%;
		color: inherit;
		cursor: pointer;
	}

	.sort-header:hover,
	.sort-header.active-sort {
		color: #cffafe;
	}

	.champion-more,
	.match-more {
		display: flex;
		width: 100%;
		height: 38px;
		align-items: center;
		justify-content: center;
		gap: 6px;
		border: 1px solid var(--color-app-border);
		border-top: 0;
		background: var(--color-app-control);
		color: #94a3b8;
		font-size: 12px;
		font-weight: 700;
		cursor: pointer;
	}

	.champion-more:hover,
	.match-more:hover:not(:disabled) {
		color: #cffafe;
		background: rgb(34 211 238 / 0.07);
	}

	.match-more {
		margin-top: 8px;
		border-top: 1px solid var(--color-app-border);
	}

	.match-more:disabled {
		cursor: wait;
		opacity: 0.65;
	}

	.load-more-error {
		margin: 8px 0 0;
		color: #fda4af;
		font-size: 11px;
		font-weight: 700;
		text-align: center;
	}

	.table-champion-icon {
		display: block;
		width: 30px;
		height: 30px;
		flex: 0 0 30px;
		object-fit: cover;
	}

	.empty-table,
	.empty-matches {
		margin: 0;
		padding: 28px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
		color: #64748b;
		font-size: 12px;
		text-align: center;
	}

	.match-heading {
		padding-bottom: 8px;
	}

	.match-filters {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.match-filters button {
		height: 30px;
		min-width: 64px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-control);
		color: #94a3b8;
		font-size: 12px;
		font-weight: 700;
		cursor: pointer;
	}

	.match-filters button.active {
		border-color: rgb(34 211 238 / 0.45);
		background: rgb(34 211 238 / 0.1);
		color: #cffafe;
	}

	.summary-grid {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		min-height: 100px;
		margin-top: 10px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}

	.summary-block {
		display: grid;
		align-content: center;
		gap: 10px;
		padding: 18px;
		border-right: 1px solid var(--color-app-border);
	}

	.summary-block:last-child {
		border-right: 0;
	}

	.summary-block strong {
		color: #f8fafc;
		font-size: 26px;
		line-height: 1;
	}

	.summary-block span {
		color: #67e8f9;
		font-size: 11px;
		font-weight: 700;
	}

	.match-list {
		min-width: 980px;
		margin-top: 10px;
	}

	.match-row {
		display: grid;
		grid-template-columns: 64px 98px 112px 104px 220px minmax(0, 1fr) 36px;
		min-height: 96px;
		border-left: 4px solid #5b8def;
		background: #16243b;
	}

	.match-row.loss-row {
		border-left-color: #d96a7c;
		background: #321f2a;
	}

	.match-row > span {
		display: grid;
		align-content: center;
		gap: 5px;
		padding: 10px;
		border-right: 1px solid rgb(0 0 0 / 0.16);
		color: #e2e8f0;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}

	.match-row b {
		color: #f8fafc;
		font-size: 12px;
	}

	.match-row small {
		color: #94a3b8;
		font-size: 11px;
	}

	.match-champion {
		position: relative;
		display: grid;
		place-items: center;
		border-right: 1px solid rgb(0 0 0 / 0.16);
	}

	.match-champion img {
		display: block;
		width: 48px;
		height: 48px;
		object-fit: cover;
	}

	.match-champion span {
		position: absolute;
		right: 8px;
		bottom: 16px;
		min-width: 16px;
		padding: 1px 3px;
		background: rgb(0 0 0 / 0.78);
		color: #f8fafc;
		font-size: 9px;
		text-align: center;
	}

	.spell-icons,
	.item-icons {
		display: flex !important;
		align-items: center;
		gap: 3px !important;
	}

	.spell-icons {
		padding: 0 !important;
	}

	.spell-icons img,
	.item-icons img {
		display: block;
		width: 24px;
		height: 24px;
		object-fit: cover;
	}

	.item-icons {
		align-content: center !important;
		flex-wrap: wrap;
		padding: 10px !important;
		border-right: 1px solid rgb(0 0 0 / 0.16);
	}

	.match-toggle {
		display: grid;
		place-items: center;
		padding: 0;
		border: 0;
		background: transparent;
		color: #94a3b8;
		cursor: pointer;
	}

	.match-entry + .match-entry {
		margin-top: 2px;
	}

	.expanded-match {
		border: 1px solid var(--color-app-border);
		border-top: 0;
		background: #111827;
	}

	.team-summary {
		display: grid;
		grid-template-columns: 170px 220px minmax(0, 1fr);
		align-items: center;
		min-height: 42px;
		padding: 0 16px;
		font-size: 11px;
	}

	.team-summary strong {
		color: #f8fafc;
	}

	.team-summary span {
		color: #cbd5e1;
	}

	.blue-summary {
		background: #263653;
	}

	.red-summary {
		background: #4a2638;
	}

	.expanded-player-row {
		display: grid;
		grid-template-columns:
			52px minmax(150px, 1.2fr) 50px 42px 82px 54px minmax(210px, 1.6fr)
			104px 92px 92px 72px;
		align-items: center;
		min-height: 72px;
		border-bottom: 1px solid rgb(148 163 184 / 0.18);
		font-variant-numeric: tabular-nums;
	}

	.expanded-blue {
		background: #1c2942;
	}

	.expanded-red {
		background: #3a202f;
	}

	.expanded-player-row > * {
		min-width: 0;
		padding: 6px;
		border-right: 1px solid rgb(148 163 184 / 0.12);
	}

	.expanded-position {
		justify-self: center;
		color: #a5f3fc;
		font-size: 10px;
	}

	.expanded-identity {
		display: grid;
		grid-template-columns: 48px minmax(0, 1fr);
		align-items: center;
		gap: 8px;
	}

	.expanded-identity img {
		display: block;
		width: 48px;
		height: 48px;
		object-fit: cover;
	}

	.expanded-identity p,
	.expanded-kda,
	.expanded-farm,
	.expanded-meter,
	.expanded-vision {
		display: grid;
		gap: 3px;
		margin: 0;
	}

	.expanded-identity strong {
		overflow: hidden;
		color: #f8fafc;
		font-size: 12px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.expanded-identity span,
	.expanded-kda span,
	.expanded-farm span,
	.expanded-vision span {
		color: #94a3b8;
		font-size: 11px;
	}

	.expanded-spells {
		display: grid;
		grid-template-columns: repeat(2, 22px);
		gap: 2px;
	}

	.expanded-spells img,
	.expanded-items img {
		display: block;
		width: 22px;
		height: 22px;
		object-fit: cover;
	}

	.expanded-level,
	.expanded-kill-rate {
		color: #cbd5e1;
		font-size: 12px;
		text-align: center;
	}

	.expanded-kda {
		text-align: center;
	}

	.expanded-kda strong {
		color: #4ade80;
		font-size: 14px;
	}

	.expanded-items {
		display: grid;
		grid-template-columns: repeat(7, 26px);
		gap: 2px;
	}

	.expanded-items img {
		width: 26px;
		height: 26px;
	}

	.expanded-farm strong,
	.expanded-meter strong,
	.expanded-vision strong {
		color: #e2e8f0;
		font-size: 12px;
	}

	.expanded-meter span {
		display: block;
		height: 7px;
		border: 1px solid #475569;
		background: #0f172a;
	}

	.expanded-meter i {
		display: block;
		height: 100%;
		background: #ef4444;
	}

	.expanded-meter.taken i {
		background: #22c55e;
	}

	@media (max-width: 720px) {
		.records-page {
			padding: 0 0 40px;
		}

		.records-shell {
			padding: 8px;
		}

		.profile-layout {
			grid-template-columns: 80px minmax(0, 1fr);
		}

		.profile-avatar {
			width: 80px;
		}

		.profile-name-line {
			font-size: 22px;
		}

		.section-heading,
		.match-heading {
			align-items: flex-start;
			flex-direction: column;
			padding: 10px 0;
		}

		.summary-grid {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}

		.summary-block {
			padding: 12px;
		}

		.summary-block strong {
			font-size: 20px;
		}
	}
</style>
