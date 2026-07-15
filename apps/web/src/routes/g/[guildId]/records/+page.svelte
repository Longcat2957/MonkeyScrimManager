<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { navigating } from '$app/state';
	import { ChevronDown, Search, TriangleAlert, UserRound } from 'lucide-svelte';
	import { fetchGuildRecords, GuildRecordsApiError } from '$lib/api/guildRecords';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import PositionIcon from '$lib/components/workflow/PositionIcon.svelte';
	import type {
		GuildRecordsGame,
		GuildRecordsLineupPlayer,
		GuildRecordsRecord,
		GuildRecordsSearchSnapshot,
		GuildRecordsSeriesRecord,
		GuildRecordsTournamentRecord,
		GuildRecordsTournamentSeries
	} from '$lib/contracts/workflow';
	import { findChampion, riotProfileIconUrl } from '$lib/datadragon';
	import type { Lane } from '$lib/fixtures/workflow';
	import { formatRelativeTime } from '$lib/workflow/presentation';
	import { untrack } from 'svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	type MatchTypeFilter = 'ALL' | 'REGULAR' | 'AUCTION';
	type LaneFilter = 'ALL' | Lane;

	const laneOrder: Lane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const laneLabel: Record<Lane, string> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '원딜',
		SUPPORT: '서폿'
	};

	function incomingSnapshotKey(): string {
		const snapshot = data.snapshot;
		return [
			data.guildId,
			data.query,
			snapshot?.selectedSeasonId ?? '',
			snapshot?.records[0]?.id ?? '',
			snapshot?.nextCursor ?? '',
			snapshot?.summary.games ?? ''
		].join('|');
	}

	const initialSnapshot = untrack(() => data.snapshot);
	let recordSnapshot = $state<GuildRecordsSearchSnapshot | null>(initialSnapshot);
	let renderedSnapshotKey = $state(untrack(incomingSnapshotKey));
	let typeFilter = $state<MatchTypeFilter>('ALL');
	let laneFilter = $state<LaneFilter>('ALL');
	let expandedRecordId = $state<string | null>(null);
	let selectedGameIndex = $state(0);
	let selectedTournamentSeriesId = $state('');
	let nextCursor = $state<string | null>(initialSnapshot?.nextCursor ?? null);
	let consumedCursors = $state<string[]>([]);
	let loadingMore = $state(false);
	let loadMoreError = $state('');

	$effect(() => {
		const nextKey = incomingSnapshotKey();
		if (nextKey === renderedSnapshotKey) return;
		renderedSnapshotKey = nextKey;
		recordSnapshot = data.snapshot;
		nextCursor = data.snapshot?.nextCursor ?? null;
		consumedCursors = [];
		typeFilter = 'ALL';
		laneFilter = 'ALL';
		expandedRecordId = null;
		selectedGameIndex = 0;
		selectedTournamentSeriesId = '';
		loadMoreError = '';
		loadingMore = false;
	});

	const guild = $derived(data.guild);
	const profile = $derived(recordSnapshot);
	const isNavigating = $derived(navigating.to !== null);
	const hasActiveSeason = $derived(
		profile?.seasons.some((season) => season.status === 'ACTIVE') ?? false
	);
	const dataDragonVersion = $derived(data.championCatalog.version);
	const searchedUserName = $derived(profile?.member.displayName ?? data.query);
	const visibleRiotAccounts = $derived(
		profile
			? [...profile.riotAccounts].sort((left, right) => Number(right.isMain) - Number(left.isMain))
			: []
	);
	const selectedSeasonName = $derived(
		profile?.seasons.find((season) => season.id === profile.selectedSeasonId)?.name ?? '시즌 없음'
	);
	const searchedMainLaneLabel = $derived(
		profile?.member.mainLane ? laneLabel[profile.member.mainLane] : '미정'
	);
	const filteredRecords = $derived(
		(recordSnapshot?.records ?? []).filter(
			(record) => recordMatchesType(record) && recordMatchesLane(record)
		)
	);

	function recordMatchesType(record: GuildRecordsRecord): boolean {
		if (typeFilter === 'ALL') return true;
		if (typeFilter === 'REGULAR') return record.kind === 'SERIES' && record.source === 'REGULAR';
		return record.kind === 'TOURNAMENT' || record.source === 'AUCTION';
	}

	function recordMatchesLane(record: GuildRecordsRecord): boolean {
		if (laneFilter === 'ALL') return true;
		if (record.kind === 'SERIES') {
			return record.games.some((game) => game.target.lane === laneFilter);
		}
		return record.series.some((series) =>
			series.games.some((game) => game.target.lane === laneFilter)
		);
	}

	function championIconUrl(id: string | null): string | null {
		if (!id) return null;
		const champion = findChampion(data.championCatalog, id);
		return champion
			? '/dd/cdn/' + dataDragonVersion + '/img/champion/' + champion.id + '.png'
			: null;
	}

	function championName(id: string | null): string {
		if (!id) return '챔피언 미입력';
		return findChampion(data.championCatalog, id)?.name ?? id;
	}

	function preferredChampion(id: string) {
		return findChampion(data.championCatalog, id);
	}

	function signed(value: number): string {
		const rounded = Math.round(value);
		return (rounded > 0 ? '+' : '') + rounded;
	}

	function mmrDeltaLabel(value: number | null): string {
		return value === null ? 'MMR 기록 없음' : 'MMR ' + signed(value);
	}

	function gameMmrDelta(game: GuildRecordsGame): number | null {
		if (game.target.mmrBefore === null || game.target.mmrAfter === null) return null;
		return game.target.mmrAfter - game.target.mmrBefore;
	}

	function resultLabel(result: 'WIN' | 'LOSS' | 'DRAW'): string {
		if (result === 'WIN') return '승리';
		if (result === 'LOSS') return '패배';
		return '무승부';
	}

	function stageLabel(stage: 'SEMIFINAL_A' | 'SEMIFINAL_B' | 'FINAL' | null): string {
		if (stage === 'SEMIFINAL_A') return '준결승 A';
		if (stage === 'SEMIFINAL_B') return '준결승 B';
		if (stage === 'FINAL') return '결승';
		return '';
	}

	function gameResult(game: GuildRecordsGame): 'WIN' | 'LOSS' {
		return game.winningSide === game.target.side ? 'WIN' : 'LOSS';
	}

	function toggleSeries(series: GuildRecordsSeriesRecord) {
		if (expandedRecordId === series.id) {
			expandedRecordId = null;
			return;
		}
		expandedRecordId = series.id;
		selectedTournamentSeriesId = '';
		selectedGameIndex = Math.max(0, series.games.length - 1);
	}

	function toggleTournament(tournament: GuildRecordsTournamentRecord) {
		if (expandedRecordId === tournament.id) {
			expandedRecordId = null;
			return;
		}
		const selected = tournament.series.at(-1);
		expandedRecordId = tournament.id;
		selectedTournamentSeriesId = selected?.id ?? '';
		selectedGameIndex = Math.max(0, (selected?.games.length ?? 1) - 1);
	}

	function selectTournamentSeries(series: GuildRecordsTournamentSeries) {
		selectedTournamentSeriesId = series.id;
		selectedGameIndex = Math.max(0, series.games.length - 1);
	}

	function activeTournamentSeries(
		tournament: GuildRecordsTournamentRecord
	): GuildRecordsTournamentSeries | undefined {
		return (
			tournament.series.find((series) => series.id === selectedTournamentSeriesId) ??
			tournament.series.at(-1)
		);
	}

	function seasonChanged(event: globalThis.Event) {
		const seasonId = (event.currentTarget as globalThis.HTMLSelectElement).value;
		const path = resolve('/g/[guildId]/records', { guildId: data.guildId });
		const query = 'q=' + globalThis.encodeURIComponent(data.query);
		const season = seasonId ? '&seasonId=' + globalThis.encodeURIComponent(seasonId) : '';
		void goto(path + '?' + query + season, { keepFocus: true, noScroll: true });
	}

	async function loadMoreRecords() {
		if (!recordSnapshot || !nextCursor || loadingMore) return;

		const cursor = nextCursor;
		if (consumedCursors.includes(cursor)) {
			nextCursor = null;
			loadMoreError = '전적 페이지 순환이 감지되었습니다. 페이지를 새로고침해 주세요.';
			return;
		}
		const sourceKey = renderedSnapshotKey;
		const sourceGuildId = data.guildId;
		const sourceQuery = data.query;
		const sourceSeasonId = recordSnapshot.selectedSeasonId;
		const sourceDisplayName = recordSnapshot.member.displayName;
		loadingMore = true;
		loadMoreError = '';
		try {
			const page = await fetchGuildRecords(globalThis.fetch, sourceGuildId, {
				query: sourceQuery,
				seasonId: sourceSeasonId ?? undefined,
				cursor,
				limit: 20
			});
			if (
				renderedSnapshotKey !== sourceKey ||
				data.guildId !== sourceGuildId ||
				data.query !== sourceQuery
			) {
				return;
			}
			if (
				!recordSnapshot ||
				page.selectedSeasonId !== sourceSeasonId ||
				page.member.displayName !== sourceDisplayName
			) {
				throw new GuildRecordsApiError(
					'조회 대상 또는 시즌이 변경되었습니다. 페이지를 새로고침해 주세요.',
					409,
					'record_source_changed'
				);
			}

			consumedCursors = [...consumedCursors, cursor];
			const knownIds = new Set(
				recordSnapshot.records.map((record) => record.kind + ':' + record.id)
			);
			const additions = page.records.filter(
				(record) => !knownIds.has(record.kind + ':' + record.id)
			);
			recordSnapshot = {
				...page,
				records: [...recordSnapshot.records, ...additions]
			};
			if (
				page.nextCursor === cursor ||
				(page.nextCursor && consumedCursors.includes(page.nextCursor))
			) {
				nextCursor = null;
				loadMoreError = '전적 페이지가 갱신되지 않았습니다. 페이지를 새로고침해 주세요.';
			} else {
				nextCursor = page.nextCursor;
			}
		} catch (cause) {
			if (
				renderedSnapshotKey !== sourceKey ||
				data.guildId !== sourceGuildId ||
				data.query !== sourceQuery
			) {
				return;
			}
			loadMoreError = cause instanceof Error ? cause.message : '전적을 더 불러오지 못했습니다.';
		} finally {
			loadingMore = false;
		}
	}
</script>

<svelte:head>
	<title>{searchedUserName ? searchedUserName + ' 내전 전적' : '내전 전적'} | MonkeyBot</title>
</svelte:head>

<div class="custom-records-page">
	<main
		class:route-loading={isNavigating}
		class="custom-records-shell"
		aria-busy={isNavigating || loadingMore}
	>
		{#if profile}
			<section class="profile-section">
				{#if profile.member.avatarUrl}
					<img
						class="profile-avatar"
						src={profile.member.avatarUrl}
						alt={searchedUserName + ' Discord 프로필'}
					/>
				{:else}
					<div class="profile-avatar profile-avatar-fallback" aria-hidden="true">
						<UserRound size={38} strokeWidth={1.6} />
					</div>
				{/if}
				<div class="profile-copy">
					<p>{guild.displayName} · 내전 참가자</p>
					<h1>{searchedUserName}</h1>
					{#if visibleRiotAccounts.length > 0}
						<ul class="linked-riot-accounts" aria-label="연결된 Riot ID">
							{#each visibleRiotAccounts as account (account.platform + ':' + account.gameName + '#' + account.tagLine)}
								<li class:main-account={account.isMain}>
									{#if account.profileIconId !== null}
										<img
											src={riotProfileIconUrl(account.profileIconId, dataDragonVersion)}
											alt=""
										/>
									{/if}
									<span>{account.gameName}#{account.tagLine}</span>
									{#if account.isMain}<strong>대표</strong>{/if}
								</li>
							{/each}
						</ul>
					{/if}
				</div>
				<div class="profile-actions">
					<label class="season-control">
						<span>시즌</span>
						<select
							value={profile.selectedSeasonId ?? ''}
							onchange={seasonChanged}
							aria-label="내전 시즌"
							disabled={profile.seasons.length === 0 || isNavigating}
						>
							{#if profile.seasons.length === 0}
								<option value="">시즌 없음</option>
							{:else}
								{#if !hasActiveSeason}
									<option value="">활성 시즌 없음</option>
								{/if}
								{#each profile.seasons as season (season.id)}
									<option value={season.id}>{season.name}</option>
								{/each}
							{/if}
						</select>
					</label>
				</div>
			</section>

			<section class="preferred-champions" aria-labelledby="preferred-champions-title">
				<header>
					<h2 id="preferred-champions-title">선호 챔피언</h2>
				</header>
				<div class="preferred-lane-grid">
					{#each laneOrder as lane (lane)}
						<div class="preferred-lane-cell">
							<div class="preferred-lane-heading">
								<PositionIcon role={lane} size="sm" />
								<strong>{laneLabel[lane]}</strong>
							</div>
							<div class="preferred-champion-list">
								{#each profile.championPreferences[lane] as championId, index (championId)}
									{@const champion = preferredChampion(championId)}
									{#if champion}
										<span
											class="preferred-champion"
											title={index + 1 + '. ' + champion.name}
											aria-label={index + 1 + '순위 ' + champion.name}
										>
											<ChampionFace {champion} version={dataDragonVersion} size="md" />
										</span>
									{:else}
										<span
											class="preferred-champion-missing"
											title={index + 1 + '. ' + championId}
											aria-label={index + 1 + '순위 ' + championId + ' (이미지 없음)'}
											>{championId}</span
										>
									{/if}
								{:else}
									<span class="preferred-champion-empty">등록 없음</span>
								{/each}
							</div>
						</div>
					{/each}
				</div>
			</section>

			<section class="lane-mmr-grid" aria-label="라인별 MMR">
				{#each profile.laneMmr as lane (lane.lane)}
					<div class="lane-mmr-cell">
						<PositionIcon role={lane.lane} size="md" />
						<div>
							<span>{laneLabel[lane.lane]}</span>
							<strong>{Math.round(lane.mmr)}</strong>
						</div>
						<p>{lane.games}게임 <b>{lane.wins}승</b></p>
					</div>
				{/each}
			</section>

			<section class="record-summary">
				<div>
					<span>승패</span>
					<strong>{profile.summary.wins}승 {profile.summary.losses}패</strong>
				</div>
				<div><span>주 포지션</span><strong>{searchedMainLaneLabel}</strong></div>
			</section>

			<section class="matches-section">
				<div class="filter-toolbar">
					<div class="filter-group">
						<button
							type="button"
							class:active={typeFilter === 'ALL'}
							onclick={() => (typeFilter = 'ALL')}>전체</button
						>
						<button
							type="button"
							class:active={typeFilter === 'REGULAR'}
							onclick={() => (typeFilter = 'REGULAR')}>일반 내전</button
						>
						<button
							type="button"
							class:active={typeFilter === 'AUCTION'}
							onclick={() => (typeFilter = 'AUCTION')}>경매 내전</button
						>
					</div>
					<div class="filter-group lane-filter">
						<button
							type="button"
							class:active={laneFilter === 'ALL'}
							onclick={() => (laneFilter = 'ALL')}>전체 라인</button
						>
						{#each laneOrder as lane (lane)}
							<button
								type="button"
								class:active={laneFilter === lane}
								onclick={() => (laneFilter = lane)}>{laneLabel[lane]}</button
							>
						{/each}
					</div>
				</div>

				<div class="match-list-scroll">
					<div class="match-list">
						{#each filteredRecords as record (record.kind + ':' + record.id)}
							{#if record.kind === 'SERIES'}
								<div class="match-entry record-entry">
									<div
										class:loss={record.result === 'LOSS'}
										class:auction={record.source === 'AUCTION'}
										class="match-row"
									>
										<div class="result-cell">
											<strong>{record.source === 'REGULAR' ? '일반' : '경매'}</strong>
											<span>{record.participantCount}인 · {resultLabel(record.result)}</span>
										</div>
										<div class="series-cell">
											<strong>{record.title}</strong>
											{#if record.opponentName || record.stage}
												<small
													>{stageLabel(record.stage)}{record.stage && record.opponentName
														? ' · '
														: ''}{record.opponentName ? 'vs ' + record.opponentName : ''}</small
												>
											{/if}
										</div>
										<div class="score-cell">
											<strong>{record.wins} : {record.losses}</strong>
										</div>
										<div class="mmr-cell series-mmr-cell">
											<strong class:negative={(record.mmrDelta ?? 0) < 0}
												>{mmrDeltaLabel(record.mmrDelta)}</strong
											>
										</div>
										<div class="date-cell">{formatRelativeTime(record.playedAt)}</div>
										<button
											type="button"
											class="expand-button"
											aria-label={record.title + ' 상세'}
											aria-expanded={expandedRecordId === record.id}
											onclick={() => toggleSeries(record)}
										>
											<ChevronDown
												size={16}
												class={expandedRecordId === record.id ? 'expanded' : ''}
												aria-hidden="true"
											/>
										</button>
									</div>

									{#if expandedRecordId === record.id}
										<div class="expanded-match">
											{#if record.games.length > 0}
												<div class="set-selector">
													{#each record.games as game, index (game.id)}
														{@const targetResult = gameResult(game)}
														{@const targetMmrDelta = gameMmrDelta(game)}
														{@const targetChampionIconUrl = championIconUrl(game.target.championId)}
														<button
															type="button"
															class:active={selectedGameIndex === index}
															onclick={() => (selectedGameIndex = index)}
														>
															<span class:set-loss={targetResult === 'LOSS'}
																>{resultLabel(targetResult)}</span
															>
															<strong>{game.gameNumber}세트</strong>
															{#if targetChampionIconUrl}
																<img
																	src={targetChampionIconUrl}
																	alt={championName(game.target.championId)}
																/>
															{:else}
																<span class="champion-slot-empty" aria-label="챔피언 미입력">-</span
																>
															{/if}
															<PositionIcon role={game.target.lane} size="sm" />
															<small>{mmrDeltaLabel(targetMmrDelta)}</small>
														</button>
													{/each}
												</div>
												{@const selectedGame = record.games[selectedGameIndex] ?? record.games[0]}
												{#if selectedGame}
													<div class="lineup-grid">
														{@render TeamLineup(
															'블루팀',
															selectedGame.blueTeam,
															selectedGame.winningSide === 'BLUE'
														)}
														{@render TeamLineup(
															'레드팀',
															selectedGame.redTeam,
															selectedGame.winningSide === 'RED'
														)}
													</div>
													{@render PickBans(selectedGame)}
												{/if}
											{:else}
												<div class="record-detail-empty">확정된 세트 기록이 없습니다.</div>
											{/if}
										</div>
									{/if}
								</div>
							{:else}
								<div class="auction-entry record-entry">
									<button
										type="button"
										class="auction-row"
										aria-label={record.title + ' 상세'}
										aria-expanded={expandedRecordId === record.id}
										onclick={() => toggleTournament(record)}
									>
										<div class="auction-type-cell"><strong>경매</strong><span>20인</span></div>
										<div class="auction-event-cell">
											<strong>{record.title}</strong>
											{#if record.teamName || record.spentPoints !== null}
												<span
													>{record.teamName ?? '팀 정보 없음'}{record.spentPoints !== null
														? ' · ' + record.spentPoints + 'P'
														: ''}</span
												>
											{/if}
										</div>
										<div class="auction-score-cell">
											<strong>{record.placement === null ? '-' : record.placement + '위'}</strong>
											<span>세트 {record.wins}승 {record.losses}패</span>
										</div>
										<div class="auction-summary-cell">
											<span>시리즈 {record.series.length}개</span>
											<strong class:negative={(record.mmrDelta ?? 0) < 0}
												>{mmrDeltaLabel(record.mmrDelta)}</strong
											>
										</div>
										<div class="date-cell">{formatRelativeTime(record.playedAt)}</div>
										<span class="expand-button" aria-hidden="true">
											<ChevronDown
												size={16}
												class={expandedRecordId === record.id ? 'expanded' : ''}
											/>
										</span>
									</button>

									{#if expandedRecordId === record.id}
										<div class="expanded-tournament">
											{#if record.roster.length > 0}
												<div class="auction-team-summary">
													<div class="auction-roster">
														{#each record.roster as player (player.displayName + ':' + player.lane)}
															{@const playerChampionIconUrl = championIconUrl(player.championId)}
															<div class:target-player={player.isTarget}>
																<PositionIcon role={player.lane} size="sm" />
																{#if playerChampionIconUrl}
																	<img
																		src={playerChampionIconUrl}
																		alt={championName(player.championId)}
																	/>
																{:else}
																	<span
																		class="champion-placeholder"
																		aria-label={championName(player.championId)}>-</span
																	>
																{/if}
																<span>
																	<strong>{player.displayName}</strong><small
																		>{laneLabel[player.lane]}</small
																	>
																</span>
															</div>
														{/each}
													</div>
												</div>
											{/if}

											{#if record.series.length > 0}
												{@const selectedSeries = activeTournamentSeries(record)}
												{#if selectedSeries}
													<div class="auction-series-selector">
														{#each record.series as series (series.id)}
															<button
																type="button"
																class:active={selectedSeries.id === series.id}
																class:loss={series.result === 'LOSS'}
																onclick={() => selectTournamentSeries(series)}
															>
																<span>{stageLabel(series.stage)}</span>
																<strong
																	>{series.opponentName
																		? 'vs ' + series.opponentName
																		: series.title}</strong
																>
																<b>{series.wins} : {series.losses}</b>
																<small>{mmrDeltaLabel(series.mmrDelta)}</small>
															</button>
														{/each}
													</div>

													<div class="auction-set-detail">
														{#if selectedSeries.games.length > 0}
															<div class="set-selector">
																{#each selectedSeries.games as game, index (game.id)}
																	{@const targetResult = gameResult(game)}
																	{@const targetMmrDelta = gameMmrDelta(game)}
																	{@const targetChampionIconUrl = championIconUrl(
																		game.target.championId
																	)}
																	<button
																		type="button"
																		class:active={selectedGameIndex === index}
																		onclick={() => (selectedGameIndex = index)}
																	>
																		<span class:set-loss={targetResult === 'LOSS'}
																			>{resultLabel(targetResult)}</span
																		>
																		<strong>{game.gameNumber}세트</strong>
																		{#if targetChampionIconUrl}
																			<img
																				src={targetChampionIconUrl}
																				alt={championName(game.target.championId)}
																			/>
																		{:else}
																			<span class="champion-slot-empty" aria-label="챔피언 미입력"
																				>-</span
																			>
																		{/if}
																		<PositionIcon role={game.target.lane} size="sm" />
																		<small>{mmrDeltaLabel(targetMmrDelta)}</small>
																	</button>
																{/each}
															</div>
															{@const selectedGame =
																selectedSeries.games[selectedGameIndex] ?? selectedSeries.games[0]}
															{#if selectedGame}
																<div class="lineup-grid">
																	{@render TeamLineup(
																		'블루팀',
																		selectedGame.blueTeam,
																		selectedGame.winningSide === 'BLUE'
																	)}
																	{@render TeamLineup(
																		'레드팀',
																		selectedGame.redTeam,
																		selectedGame.winningSide === 'RED'
																	)}
																</div>
																{@render PickBans(selectedGame)}
															{/if}
														{:else}
															<div class="record-detail-empty">확정된 세트 기록이 없습니다.</div>
														{/if}
													</div>
												{/if}
											{:else}
												<div class="record-detail-empty">완료된 시리즈 기록이 없습니다.</div>
											{/if}
										</div>
									{/if}
								</div>
							{/if}
						{:else}
							<div class="record-history-empty">
								<strong>
									{profile.records.length === 0
										? selectedSeasonName + '에 확정된 내전 전적이 없습니다.'
										: '선택한 조건에 맞는 내전 전적이 없습니다.'}
								</strong>
								<span>확정된 경기 결과만 표시됩니다.</span>
							</div>
						{/each}
					</div>
				</div>

				{#if loadMoreError}
					<p class="records-inline-error" role="alert">{loadMoreError}</p>
				{/if}
				{#if nextCursor}
					<div class="records-load-more">
						<button type="button" disabled={loadingMore} onclick={loadMoreRecords}>
							{loadingMore ? '불러오는 중' : '더 보기'}
						</button>
					</div>
				{/if}
			</section>

			<section class="champion-records">
				<header>
					<h2>챔피언 사용 기록</h2>
					<span>{selectedSeasonName}</span>
				</header>
				<div class="champion-table-scroll">
					<div class="champion-table">
						<div class="champion-table-row champion-table-head">
							<span>챔피언</span><span>전적</span><span>주 포지션</span>
						</div>
						{#each profile.championRecords as champion (champion.championId + ':' + champion.lane)}
							{@const championRecordIconUrl = championIconUrl(champion.championId)}
							<div class="champion-table-row">
								<span>
									{#if championRecordIconUrl}<img src={championRecordIconUrl} alt="" />{/if}
									{championName(champion.championId)}
								</span>
								<span>{champion.wins}승 {champion.losses}패</span>
								<span
									><PositionIcon role={champion.lane} size="sm" />{laneLabel[champion.lane]}</span
								>
							</div>
						{:else}
							<div class="champion-table-empty">이 시즌의 챔피언 사용 기록이 없습니다.</div>
						{/each}
					</div>
				</div>
			</section>
		{:else if data.loadError}
			<section class="records-empty-state records-error-state" role="alert">
				<TriangleAlert size={28} aria-hidden="true" />
				<h1>내전 전적을 불러오지 못했습니다</h1>
				<p>{data.loadError}</p>
			</section>
		{:else}
			<section class="records-empty-state">
				<Search size={28} aria-hidden="true" />
				<h1>내전 전적을 확인할 사용자를 검색해 주세요</h1>
				<p>현재 길드의 Discord 이름 또는 공개된 Riot ID로 참가자를 찾을 수 있습니다.</p>
			</section>
		{/if}
	</main>
</div>

{#snippet TeamLineup(label: string, players: GuildRecordsLineupPlayer[], winner: boolean)}
	<section class:winner class="team-lineup">
		<header>
			<h3>{label}</h3>
			{#if winner}<span>승리</span>{/if}
		</header>
		<div>
			{#each players as player (player.displayName + ':' + player.lane)}
				{@const playerChampionIconUrl = championIconUrl(player.championId)}
				<div class:target-player={player.isTarget} class="lineup-player">
					<PositionIcon role={player.lane} size="sm" />
					{#if playerChampionIconUrl}
						<img src={playerChampionIconUrl} alt={championName(player.championId)} />
					{:else}
						<span class="champion-placeholder" aria-hidden="true">-</span>
					{/if}
					<div>
						<strong>{player.displayName}</strong><span>{championName(player.championId)}</span>
					</div>
				</div>
			{:else}
				<div class="lineup-empty">라인업 기록 없음</div>
			{/each}
		</div>
	</section>
{/snippet}

{#snippet PickBans(game: GuildRecordsGame)}
	{#if game.blueBans.length > 0 || game.redBans.length > 0}
		<div class="pick-ban-row">
			<span>블루 밴</span>
			<div>
				{#each game.blueBans as champion (champion)}
					{@const banIconUrl = championIconUrl(champion)}
					{#if banIconUrl}
						<img src={banIconUrl} alt={championName(champion)} />
					{:else}
						<span class="ban-placeholder">{champion}</span>
					{/if}
				{/each}
			</div>
			<span>레드 밴</span>
			<div>
				{#each game.redBans as champion (champion)}
					{@const banIconUrl = championIconUrl(champion)}
					{#if banIconUrl}
						<img src={banIconUrl} alt={championName(champion)} />
					{:else}
						<span class="ban-placeholder">{champion}</span>
					{/if}
				{/each}
			</div>
		</div>
	{/if}
{/snippet}

<style>
	.custom-records-page {
		min-height: calc(100vh - 57px);
		padding: 0 16px 48px;
		background: var(--color-app-bg);
		color: #e2e8f0;
	}
	.custom-records-shell {
		width: min(100%, 1280px);
		margin: 0 auto;
		padding: 0 12px 12px;
		background: var(--color-app-bg-soft);
		transition: opacity 140ms ease;
	}
	.custom-records-shell.route-loading {
		opacity: 0.65;
		pointer-events: none;
	}
	.profile-section {
		display: grid;
		grid-template-columns: 96px minmax(0, 1fr) auto;
		align-items: center;
		gap: 16px;
		padding: 14px;
		background: var(--color-app-surface);
	}
	.profile-avatar {
		width: 96px;
		height: 96px;
		object-fit: cover;
	}
	.profile-avatar-fallback {
		display: grid;
		place-items: center;
		background: var(--color-app-control);
		color: #64748b;
	}
	.profile-copy {
		min-width: 0;
	}
	.profile-copy > p {
		margin: 0;
		color: #94a3b8;
		font-size: 12px;
		font-weight: 600;
	}
	.profile-copy h1 {
		min-width: 0;
		margin: 5px 0;
		color: #f8fafc;
		font-size: 27px;
		line-height: 1.1;
		overflow-wrap: anywhere;
	}
	.profile-actions {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.linked-riot-accounts {
		display: flex;
		min-width: 0;
		flex-wrap: wrap;
		gap: 5px;
		margin: 8px 0 0;
		padding: 0;
		list-style: none;
	}
	.linked-riot-accounts li {
		display: inline-flex;
		min-width: 0;
		height: 28px;
		align-items: center;
		gap: 6px;
		background: var(--color-app-control);
		padding: 3px 8px 3px 4px;
		color: #94a3b8;
		font-size: 11px;
		font-weight: 700;
	}
	.linked-riot-accounts li.main-account {
		background: rgb(34 211 238 / 0.08);
		color: #cffafe;
	}
	.linked-riot-accounts img {
		width: 22px;
		height: 22px;
		flex: none;
		object-fit: cover;
	}
	.linked-riot-accounts li > span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.linked-riot-accounts strong {
		flex: none;
		color: #67e8f9;
		font-size: 9px;
	}
	.season-control {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: #64748b;
		font-size: 11px;
		font-weight: 700;
	}
	.season-control select {
		width: 128px;
		height: 34px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		padding: 0 10px;
		color: #e2e8f0;
		outline: none;
	}
	.preferred-champions {
		margin-top: 12px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}
	.preferred-champions > header {
		display: flex;
		min-height: 42px;
		align-items: center;
		padding: 0 12px;
		border-bottom: 1px solid var(--color-app-border-muted);
	}
	.preferred-champions h2 {
		margin: 0;
		color: #e2e8f0;
		font-size: 13px;
	}
	.preferred-lane-grid {
		display: grid;
		grid-template-columns: repeat(5, minmax(0, 1fr));
		gap: 1px;
		background: var(--color-app-border-muted);
	}
	.preferred-lane-cell {
		min-width: 0;
		min-height: 94px;
		background: var(--color-app-surface);
		padding: 10px;
	}
	.preferred-lane-heading {
		display: flex;
		align-items: center;
		gap: 7px;
		color: #cbd5e1;
		font-size: 11px;
	}
	.preferred-champion-list {
		display: flex;
		min-width: 0;
		flex-wrap: wrap;
		align-items: center;
		gap: 5px;
		margin-top: 9px;
	}
	.preferred-champion {
		display: inline-flex;
	}
	.preferred-champion-missing {
		display: inline-flex;
		max-width: 100%;
		height: 40px;
		align-items: center;
		overflow: hidden;
		background: var(--color-app-control);
		padding: 0 8px;
		color: #94a3b8;
		font-size: 10px;
		font-weight: 700;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.preferred-champion-empty {
		color: #64748b;
		font-size: 10px;
	}
	.lane-mmr-grid {
		display: grid;
		grid-template-columns: repeat(5, minmax(0, 1fr));
		margin-top: 12px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}
	.lane-mmr-cell {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		align-items: center;
		gap: 10px;
		min-height: 86px;
		padding: 12px;
		border-right: 1px solid var(--color-app-border-muted);
	}
	.lane-mmr-cell:last-child {
		border-right: 0;
	}
	.lane-mmr-cell div {
		display: grid;
		gap: 2px;
	}
	.lane-mmr-cell span {
		color: #94a3b8;
		font-size: 11px;
		font-weight: 700;
	}
	.lane-mmr-cell strong {
		color: #f8fafc;
		font-size: 20px;
	}
	.lane-mmr-cell p {
		grid-column: 1 / -1;
		margin: 0;
		color: #64748b;
		font-size: 11px;
	}
	.lane-mmr-cell b,
	.mmr-cell > strong {
		margin-left: 4px;
		color: #67e8f9;
	}
	.negative {
		color: #fda4af !important;
	}
	.record-summary {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		margin-top: 12px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}
	.record-summary > div {
		display: grid;
		align-content: center;
		gap: 5px;
		min-height: 78px;
		padding: 12px 16px;
		border-right: 1px solid var(--color-app-border-muted);
	}
	.record-summary > div:last-child {
		border-right: 0;
	}
	.record-summary span {
		color: #64748b;
		font-size: 11px;
		font-weight: 700;
	}
	.record-summary strong {
		color: #f8fafc;
		font-size: 18px;
	}
	.matches-section {
		margin-top: 18px;
	}
	.filter-toolbar {
		display: flex;
		min-width: 0;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		min-height: 50px;
		border-bottom: 1px solid var(--color-app-border);
	}
	.filter-group {
		display: flex;
		gap: 7px;
	}
	.filter-group button {
		height: 30px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		padding: 0 12px;
		color: #94a3b8;
		font-size: 12px;
		font-weight: 700;
		cursor: pointer;
	}
	.filter-group button.active {
		border-color: rgb(34 211 238 / 0.45);
		background: rgb(34 211 238 / 0.1);
		color: #cffafe;
	}
	.match-list-scroll,
	.champion-table-scroll {
		overflow-x: auto;
	}
	.match-list {
		min-width: 1040px;
		margin-top: 10px;
	}
	.match-entry + .match-entry {
		margin-top: 3px;
	}
	.match-entry + .auction-entry,
	.auction-entry + .auction-entry {
		margin-top: 3px;
	}
	.match-row {
		display: grid;
		grid-template-columns: 108px minmax(240px, 1fr) 120px 210px 90px 36px;
		min-height: 78px;
		border-left: 4px solid #38bdf8;
		background: #16243b;
	}
	.match-row.loss {
		border-left-color: #fb7185;
		background: #321f2a;
	}
	.match-row.auction {
		border-left-color: #f59e0b;
		background: #2b2417;
	}
	.match-row > * {
		min-width: 0;
		border-right: 1px solid rgb(0 0 0 / 0.16);
	}
	.result-cell,
	.mmr-cell,
	.series-cell,
	.score-cell {
		display: grid;
		align-content: center;
		gap: 4px;
		padding: 10px 12px;
	}
	.result-cell strong {
		color: #f8fafc;
		font-size: 13px;
	}
	.result-cell span {
		color: #94a3b8;
		font-size: 10px;
		font-weight: 700;
	}
	.match-row.auction .result-cell strong {
		color: #fbbf24;
	}
	.series-cell strong {
		overflow: hidden;
		color: #f8fafc;
		font-size: 12px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.series-cell small {
		overflow: hidden;
		color: #94a3b8;
		font-size: 10px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.score-cell {
		justify-items: center;
		text-align: center;
	}
	.score-cell strong {
		color: #f8fafc;
		font-size: 20px;
		font-variant-numeric: tabular-nums;
	}
	.mmr-cell {
		grid-template-columns: minmax(0, 1fr) auto;
		align-items: center;
	}
	.mmr-cell strong {
		font-size: 14px;
	}
	.series-mmr-cell {
		grid-template-columns: 1fr;
		justify-items: center;
	}
	.series-mmr-cell > strong {
		margin-left: 0;
	}
	.date-cell,
	.expand-button {
		display: grid;
		place-items: center;
		color: #94a3b8;
		font-size: 11px;
	}
	.expand-button {
		border: 0;
		background: transparent;
		cursor: pointer;
	}
	.expand-button :global(svg) {
		transition: transform 140ms ease;
	}
	.expand-button :global(svg.expanded) {
		transform: rotate(180deg);
	}
	.record-entry + .record-entry {
		margin-top: 3px;
	}
	.auction-row {
		display: grid;
		grid-template-columns: 108px minmax(240px, 1fr) 120px 210px 90px 36px;
		width: 100%;
		min-height: 78px;
		border: 0;
		border-left: 4px solid #f59e0b;
		background: #2b2417;
		padding: 0;
		color: inherit;
		text-align: left;
		cursor: pointer;
	}
	.auction-row:hover {
		background: #342a18;
	}
	.auction-row > * {
		min-width: 0;
		border-right: 1px solid rgb(0 0 0 / 0.18);
	}
	.auction-type-cell,
	.auction-event-cell,
	.auction-score-cell,
	.auction-summary-cell {
		display: grid;
		align-content: center;
		gap: 4px;
		padding: 10px 12px;
	}
	.auction-type-cell strong {
		color: #fbbf24;
		font-size: 14px;
	}
	.auction-type-cell span,
	.auction-event-cell span,
	.auction-score-cell span {
		color: #a8a29e;
		font-size: 10px;
		font-weight: 700;
	}
	.auction-event-cell strong {
		overflow: hidden;
		color: #fff7ed;
		font-size: 13px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.auction-score-cell {
		justify-items: center;
		text-align: center;
	}
	.auction-score-cell strong {
		color: #fbbf24;
		font-size: 18px;
		font-variant-numeric: tabular-nums;
	}
	.auction-summary-cell {
		grid-template-columns: minmax(0, 1fr) auto;
		align-items: center;
	}
	.auction-summary-cell span {
		color: #cbd5e1;
		font-size: 13px;
		font-variant-numeric: tabular-nums;
	}
	.auction-summary-cell strong {
		color: #67e8f9;
		font-size: 14px;
		font-variant-numeric: tabular-nums;
	}
	.expanded-tournament {
		border: 1px solid #574525;
		border-top: 0;
		background: #111827;
	}
	.auction-team-summary {
		border-bottom: 1px solid #574525;
		background: #1e1c18;
	}
	.auction-roster {
		display: grid;
		grid-template-columns: repeat(5, minmax(0, 1fr));
	}
	.auction-roster > div {
		display: grid;
		grid-template-columns: 22px 36px minmax(0, 1fr);
		align-items: center;
		gap: 7px;
		min-height: 52px;
		padding: 7px 10px;
		border-right: 1px solid rgb(245 158 11 / 0.14);
	}
	.auction-roster > div:last-child {
		border-right: 0;
	}
	.auction-roster > div.target-player {
		box-shadow: inset 3px 0 #f59e0b;
		background: rgb(245 158 11 / 0.06);
	}
	.auction-roster img {
		width: 36px;
		height: 36px;
		object-fit: cover;
	}
	.auction-roster span {
		display: grid;
		min-width: 0;
		gap: 2px;
	}
	.auction-roster strong {
		overflow: hidden;
		color: #f8fafc;
		font-size: 11px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.auction-roster small {
		color: #a8a29e;
		font-size: 9px;
	}
	.auction-series-selector {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 260px));
		justify-content: center;
		gap: 8px;
		padding: 10px 12px;
		border-bottom: 1px solid var(--color-app-border);
	}
	.auction-series-selector button {
		display: grid;
		grid-template-columns: 42px minmax(0, 1fr) 52px;
		align-items: center;
		gap: 8px;
		min-height: 52px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		padding: 7px 10px;
		color: #cbd5e1;
		text-align: left;
		cursor: pointer;
	}
	.auction-series-selector button.active {
		border-color: rgb(245 158 11 / 0.65);
		background: rgb(245 158 11 / 0.1);
	}
	.auction-series-selector button.loss.active {
		border-color: rgb(251 113 133 / 0.6);
		background: rgb(251 113 133 / 0.08);
	}
	.auction-series-selector span {
		color: #fbbf24;
		font-size: 10px;
		font-weight: 800;
	}
	.auction-series-selector strong {
		overflow: hidden;
		font-size: 12px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.auction-series-selector b {
		color: #f8fafc;
		font-size: 16px;
		text-align: right;
	}
	.auction-series-selector small {
		grid-column: 2 / -1;
		color: #94a3b8;
		font-size: 10px;
	}
	.auction-set-detail .set-selector {
		background: #101722;
	}
	.expanded-match {
		border: 1px solid var(--color-app-border);
		border-top: 0;
		background: #111827;
	}
	.set-selector {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 220px));
		justify-content: center;
		gap: 8px;
		padding: 10px 12px;
		border-bottom: 1px solid var(--color-app-border);
	}
	.set-selector button {
		display: grid;
		grid-template-columns: auto auto 34px 22px minmax(0, 1fr);
		align-items: center;
		gap: 7px;
		min-height: 46px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		padding: 6px 9px;
		color: #94a3b8;
		font-size: 11px;
		cursor: pointer;
	}
	.set-selector button.active {
		border-color: rgb(34 211 238 / 0.5);
		background: rgb(34 211 238 / 0.08);
	}
	.set-selector button > span {
		color: #67e8f9;
		font-size: 10px;
		font-weight: 800;
	}
	.set-selector button > span.set-loss {
		color: #fda4af;
	}
	.set-selector button > strong {
		color: #e2e8f0;
		font-size: 11px;
	}
	.set-selector button > img {
		width: 34px;
		height: 34px;
		object-fit: cover;
	}
	.set-selector button > .champion-slot-empty {
		display: grid;
		width: 34px;
		height: 34px;
		place-items: center;
		background: rgb(15 23 42 / 0.75);
		color: #64748b;
		font-size: 11px;
	}
	.set-selector button > small {
		overflow: hidden;
		font-size: 10px;
		text-align: right;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.lineup-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 1px;
		background: var(--color-app-border-muted);
	}
	.team-lineup {
		background: #17223a;
	}
	.team-lineup:last-child {
		background: #321f2a;
	}
	.team-lineup > header {
		display: flex;
		height: 40px;
		align-items: center;
		justify-content: space-between;
		padding: 0 12px;
		border-bottom: 1px solid rgb(148 163 184 / 0.16);
	}
	.team-lineup h3 {
		margin: 0;
		font-size: 12px;
	}
	.team-lineup header span {
		border-radius: 2px;
		background: rgb(34 211 238 / 0.12);
		padding: 3px 7px;
		color: #a5f3fc;
		font-size: 10px;
		font-weight: 800;
	}
	.lineup-player {
		display: grid;
		grid-template-columns: 22px 38px minmax(0, 1fr);
		align-items: center;
		gap: 8px;
		min-height: 54px;
		padding: 7px 10px;
		border-bottom: 1px solid rgb(148 163 184 / 0.1);
	}
	.lineup-player.target-player {
		box-shadow: inset 3px 0 #22d3ee;
		background: rgb(34 211 238 / 0.06);
	}
	.lineup-player > img {
		width: 38px;
		height: 38px;
		object-fit: cover;
	}
	.lineup-player > .champion-placeholder,
	.auction-roster > div > .champion-placeholder {
		display: grid;
		width: 38px;
		height: 38px;
		place-items: center;
		background: rgb(15 23 42 / 0.75);
		color: #64748b;
		font-size: 11px;
	}
	.auction-roster > div > .champion-placeholder {
		width: 36px;
		height: 36px;
	}
	.lineup-empty,
	.record-detail-empty {
		display: grid;
		min-height: 72px;
		place-items: center;
		color: #64748b;
		font-size: 11px;
	}
	.lineup-player div {
		display: grid;
		min-width: 0;
		gap: 2px;
	}
	.lineup-player strong {
		overflow: hidden;
		color: #f8fafc;
		font-size: 11px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.lineup-player span {
		color: #94a3b8;
		font-size: 10px;
	}
	.pick-ban-row {
		display: grid;
		grid-template-columns: 54px 120px 54px 120px;
		align-items: center;
		gap: 9px;
		min-height: 54px;
		padding: 8px 12px;
	}
	.pick-ban-row > span {
		color: #64748b;
		font-size: 10px;
		font-weight: 700;
	}
	.pick-ban-row > div {
		display: flex;
		gap: 3px;
	}
	.pick-ban-row img {
		width: 30px;
		height: 30px;
		object-fit: cover;
		filter: grayscale(0.75);
	}
	.pick-ban-row .ban-placeholder {
		display: grid;
		width: 30px;
		height: 30px;
		place-items: center;
		overflow: hidden;
		background: rgb(15 23 42 / 0.75);
		color: #64748b;
		font-size: 8px;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.champion-records {
		margin-top: 20px;
	}
	.champion-records > header {
		display: flex;
		height: 46px;
		align-items: center;
		justify-content: space-between;
		padding: 0 12px;
		border-bottom: 1px solid var(--color-app-border);
	}
	.champion-records h2 {
		margin: 0;
		font-size: 14px;
	}
	.champion-records header span {
		color: #64748b;
		font-size: 11px;
	}
	.champion-table {
		min-width: 900px;
		border-top: 1px solid var(--color-app-border);
		border-left: 1px solid var(--color-app-border);
	}
	.champion-table-row {
		display: grid;
		grid-template-columns: minmax(220px, 1fr) 180px 150px;
		min-height: 42px;
	}
	.champion-table-row > span {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding: 6px 10px;
		border-right: 1px solid var(--color-app-border-muted);
		border-bottom: 1px solid var(--color-app-border-muted);
		background: #111a26;
		color: #94a3b8;
		font-size: 11px;
		font-variant-numeric: tabular-nums;
	}
	.champion-table-row > span:first-child {
		justify-content: flex-start;
		color: #e2e8f0;
		font-weight: 700;
	}
	.champion-table-head > span {
		background: #1a2b35;
		color: #cbd5e1 !important;
		font-weight: 700;
	}
	.champion-table-row img {
		width: 28px;
		height: 28px;
		object-fit: cover;
	}
	.champion-table-empty {
		display: grid;
		min-height: 72px;
		place-items: center;
		border-right: 1px solid var(--color-app-border-muted);
		border-bottom: 1px solid var(--color-app-border-muted);
		background: var(--color-app-surface);
		color: var(--color-app-muted);
		font-size: 11px;
	}
	.record-history-empty {
		display: grid;
		min-height: 150px;
		place-content: center;
		justify-items: center;
		gap: 5px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
		color: var(--color-app-muted);
		font-size: 11px;
	}
	.record-history-empty strong {
		color: #cbd5e1;
		font-size: 12px;
	}
	.records-load-more {
		display: flex;
		justify-content: center;
		padding: 14px 0 0;
	}
	.records-load-more button {
		min-width: 116px;
		height: 34px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-control);
		color: #cbd5e1;
		font-size: 11px;
		font-weight: 700;
		cursor: pointer;
	}
	.records-load-more button:hover:not(:disabled) {
		border-color: rgb(34 211 238 / 0.45);
		color: #cffafe;
	}
	.records-load-more button:disabled {
		cursor: wait;
		opacity: 0.55;
	}
	.records-inline-error {
		margin: 10px 0 0;
		color: #fda4af;
		font-size: 11px;
		text-align: center;
	}
	.records-empty-state {
		display: grid;
		min-height: 420px;
		place-content: center;
		justify-items: center;
		gap: 8px;
		padding: 32px 16px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
		color: var(--color-app-muted);
		text-align: center;
	}
	.records-empty-state h1 {
		margin: 4px 0 0;
		color: #e2e8f0;
		font-size: 16px;
	}
	.records-empty-state p {
		margin: 0;
		font-size: 12px;
	}
	.records-error-state {
		color: #fb7185;
	}
	@media (max-width: 820px) {
		.custom-records-page {
			padding: 0 0 40px;
		}
		.custom-records-shell {
			padding: 0 8px 8px;
		}
		.profile-section {
			grid-template-columns: 72px minmax(0, 1fr);
		}
		.profile-avatar {
			width: 72px;
			height: 72px;
		}
		.profile-actions {
			grid-column: 1 / -1;
			justify-content: flex-end;
		}
		.preferred-lane-grid {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
		.lane-mmr-grid {
			overflow-x: auto;
			grid-template-columns: repeat(5, 180px);
		}
		.record-summary {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
		.filter-toolbar {
			align-items: flex-start;
			flex-direction: column;
			padding: 8px 0;
		}
		.lane-filter {
			max-width: 100%;
			overflow-x: auto;
		}
		.lineup-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
