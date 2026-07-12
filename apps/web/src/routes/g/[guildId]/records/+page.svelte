<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { Search } from 'lucide-svelte';
	import PositionIcon from '$lib/components/workflow/PositionIcon.svelte';
	import type { Lane } from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let recordSearch = $state('');

	type MatchType = '일반 내전' | '경매 내전';
	type MatchResult = '승리' | '패배';

	interface LaneMmr {
		lane: Lane;
		mmr: number;
		games: number;
		change: number;
	}

	interface LineupPlayer {
		name: string;
		lane: Lane;
		championId: string;
		championName: string;
		mmrChange: number;
		isTarget?: boolean;
	}

	interface CustomSet {
		label: string;
		result: MatchResult;
		lane: Lane;
		championId: string;
		championName: string;
		mmrBefore: number;
		mmrAfter: number;
		blueTeam: LineupPlayer[];
		redTeam: LineupPlayer[];
		blueBans: string[];
		redBans: string[];
	}

	interface CustomMatch {
		id: string;
		result: MatchResult;
		type: MatchType;
		participantCount: 10 | 20;
		seriesName: string;
		score: string;
		wins: number;
		losses: number;
		mmrBefore: number;
		mmrAfter: number;
		playedAt: string;
		sets: CustomSet[];
	}

	interface AuctionTournamentRecord {
		id: string;
		name: string;
		teamName: string;
		spentPoints: number;
		placement: string;
		playedAt: string;
		roster: LineupPlayer[];
		series: CustomMatch[];
		opponents: Record<string, string>;
		stages: Record<string, string>;
	}

	interface ChampionRecord {
		id: string;
		name: string;
		wins: number;
		losses: number;
		mainLane: Lane;
	}

	const dataDragonVersion = '16.13.1';
	const guild = $derived(data.guild);
	const searchedRiotId = $derived(page.url.searchParams.get('q')?.trim() || '탐솔#KR1');
	const searchedUserName = $derived(searchedRiotId.split('#')[0] || '탐솔');
	const searchedUserTag = $derived(
		searchedRiotId.includes('#') ? searchedRiotId : `${searchedUserName}#KR1`
	);
	const laneOrder: Lane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
	const laneLabel: Record<Lane, string> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '원딜',
		SUPPORT: '서폿'
	};

	let typeFilter = $state<'전체' | MatchType>('전체');
	let laneFilter = $state<'전체' | Lane>('전체');
	let expandedMatchId = $state<string | null>('custom-1');
	let selectedSetIndex = $state(2);
	let expandedTournamentId = $state<string | null>(null);
	let selectedAuctionSeriesId = $state('custom-2');
	let season = $state('시즌 3');

	const laneMmr: LaneMmr[] = [
		{ lane: 'TOP', mmr: 1518, games: 4, change: 8 },
		{ lane: 'JUNGLE', mmr: 1540, games: 3, change: -6 },
		{ lane: 'MID', mmr: 1642, games: 31, change: 42 },
		{ lane: 'BOTTOM', mmr: 1588, games: 10, change: 18 },
		{ lane: 'SUPPORT', mmr: 1510, games: 6, change: 3 }
	];

	const blueTeam: LineupPlayer[] = [
		{ name: '마루', lane: 'TOP', championId: 'Ornn', championName: '오른', mmrChange: 17 },
		{ name: '서초', lane: 'JUNGLE', championId: 'LeeSin', championName: '리 신', mmrChange: 18 },
		{
			name: '탐솔',
			lane: 'MID',
			championId: 'Ahri',
			championName: '아리',
			mmrChange: 19,
			isTarget: true
		},
		{ name: '모카', lane: 'BOTTOM', championId: 'Ezreal', championName: '이즈리얼', mmrChange: 16 },
		{
			name: '파도',
			lane: 'SUPPORT',
			championId: 'Nautilus',
			championName: '노틸러스',
			mmrChange: 15
		}
	];

	const redTeam: LineupPlayer[] = [
		{ name: '로웰', lane: 'TOP', championId: 'Renekton', championName: '레넥톤', mmrChange: -16 },
		{ name: '모노', lane: 'JUNGLE', championId: 'Viego', championName: '비에고', mmrChange: -18 },
		{ name: '유자', lane: 'MID', championId: 'Orianna', championName: '오리아나', mmrChange: -17 },
		{ name: '설탕', lane: 'BOTTOM', championId: 'Kaisa', championName: '카이사', mmrChange: -18 },
		{ name: '누리', lane: 'SUPPORT', championId: 'Rakan', championName: '라칸', mmrChange: -16 }
	];

	function teamWithTarget(
		players: LineupPlayer[],
		championId: string,
		championName: string,
		lane: Lane,
		mmrChange: number
	): LineupPlayer[] {
		return players.map((player) =>
			player.isTarget
				? { ...player, championId, championName, lane, mmrChange }
				: {
						...player,
						mmrChange: mmrChange > 0 ? Math.max(12, mmrChange - 2) : Math.min(-12, mmrChange + 2)
					}
		);
	}

	const matches: CustomMatch[] = [
		{
			id: 'custom-1',
			result: '승리',
			type: '일반 내전',
			participantCount: 10,
			seriesName: '금요 정기 내전',
			score: '2 : 1',
			wins: 2,
			losses: 1,
			mmrBefore: 1607,
			mmrAfter: 1642,
			playedAt: '3일 전',
			sets: [
				{
					label: '1세트',
					result: '패배',
					lane: 'MID',
					championId: 'Orianna',
					championName: '오리아나',
					mmrBefore: 1607,
					mmrAfter: 1591,
					blueTeam: teamWithTarget(blueTeam, 'Orianna', '오리아나', 'MID', -16),
					redTeam,
					blueBans: ['Jax', 'Vi', 'Syndra'],
					redBans: ['KSante', 'Nocturne', 'Ziggs']
				},
				{
					label: '2세트',
					result: '승리',
					lane: 'MID',
					championId: 'Sylas',
					championName: '사일러스',
					mmrBefore: 1591,
					mmrAfter: 1623,
					blueTeam: teamWithTarget(blueTeam, 'Sylas', '사일러스', 'MID', 32),
					redTeam,
					blueBans: ['Renekton', 'Viego', 'Kaisa'],
					redBans: ['Ornn', 'Ahri', 'Ezreal']
				},
				{
					label: '3세트',
					result: '승리',
					lane: 'MID',
					championId: 'Ahri',
					championName: '아리',
					mmrBefore: 1623,
					mmrAfter: 1642,
					blueTeam,
					redTeam,
					blueBans: ['Jax', 'Vi', 'Syndra'],
					redBans: ['KSante', 'Nocturne', 'Ziggs']
				}
			]
		},
		{
			id: 'custom-2',
			result: '패배',
			type: '경매 내전',
			participantCount: 20,
			seriesName: '여름 경매 내전',
			score: '1 : 2',
			wins: 1,
			losses: 2,
			mmrBefore: 1605,
			mmrAfter: 1588,
			playedAt: '5일 전',
			sets: [
				{
					label: '1세트',
					result: '승리',
					lane: 'BOTTOM',
					championId: 'Ezreal',
					championName: '이즈리얼',
					mmrBefore: 1605,
					mmrAfter: 1621,
					blueTeam: teamWithTarget(blueTeam, 'Ezreal', '이즈리얼', 'BOTTOM', 16),
					redTeam,
					blueBans: ['Ahri', 'LeeSin', 'Rakan'],
					redBans: ['Ornn', 'Kaisa', 'Viego']
				},
				{
					label: '2세트',
					result: '패배',
					lane: 'BOTTOM',
					championId: 'Kaisa',
					championName: '카이사',
					mmrBefore: 1621,
					mmrAfter: 1604,
					blueTeam: teamWithTarget(blueTeam, 'Kaisa', '카이사', 'BOTTOM', -17),
					redTeam,
					blueBans: ['Jax', 'Vi', 'Syndra'],
					redBans: ['KSante', 'Nocturne', 'Ziggs']
				},
				{
					label: '3세트',
					result: '패배',
					lane: 'BOTTOM',
					championId: 'Jinx',
					championName: '징크스',
					mmrBefore: 1604,
					mmrAfter: 1588,
					blueTeam: teamWithTarget(blueTeam, 'Jinx', '징크스', 'BOTTOM', -16),
					redTeam,
					blueBans: ['Renekton', 'Viego', 'Ahri'],
					redBans: ['Ornn', 'Ezreal', 'LeeSin']
				}
			]
		},
		{
			id: 'custom-3',
			result: '승리',
			type: '일반 내전',
			participantCount: 10,
			seriesName: '주말 자유 내전',
			score: '2 : 0',
			wins: 2,
			losses: 0,
			mmrBefore: 1592,
			mmrAfter: 1623,
			playedAt: '4일 전',
			sets: [
				{
					label: '1세트',
					result: '승리',
					lane: 'MID',
					championId: 'Orianna',
					championName: '오리아나',
					mmrBefore: 1592,
					mmrAfter: 1607,
					blueTeam: teamWithTarget(blueTeam, 'Orianna', '오리아나', 'MID', 15),
					redTeam,
					blueBans: ['Renekton', 'Vi', 'Syndra'],
					redBans: ['Ahri', 'Nocturne', 'Ziggs']
				},
				{
					label: '2세트',
					result: '승리',
					lane: 'MID',
					championId: 'Ahri',
					championName: '아리',
					mmrBefore: 1607,
					mmrAfter: 1623,
					blueTeam: teamWithTarget(blueTeam, 'Ahri', '아리', 'MID', 16),
					redTeam,
					blueBans: ['Jax', 'Kaisa', 'Viego'],
					redBans: ['Ornn', 'LeeSin', 'Ezreal']
				}
			]
		},
		{
			id: 'custom-4',
			result: '승리',
			type: '경매 내전',
			participantCount: 20,
			seriesName: '여름 경매 내전',
			score: '2 : 1',
			wins: 2,
			losses: 1,
			mmrBefore: 1478,
			mmrAfter: 1510,
			playedAt: '12일 전',
			sets: [
				{
					label: '1세트',
					result: '승리',
					lane: 'SUPPORT',
					championId: 'Nautilus',
					championName: '노틸러스',
					mmrBefore: 1478,
					mmrAfter: 1495,
					blueTeam: teamWithTarget(blueTeam, 'Nautilus', '노틸러스', 'SUPPORT', 17),
					redTeam,
					blueBans: ['Jax', 'Viego', 'Kaisa'],
					redBans: ['Ornn', 'Ahri', 'Ezreal']
				},
				{
					label: '2세트',
					result: '패배',
					lane: 'SUPPORT',
					championId: 'Rakan',
					championName: '라칸',
					mmrBefore: 1495,
					mmrAfter: 1481,
					blueTeam: teamWithTarget(blueTeam, 'Rakan', '라칸', 'SUPPORT', -14),
					redTeam,
					blueBans: ['Renekton', 'Vi', 'Syndra'],
					redBans: ['KSante', 'Nocturne', 'Ziggs']
				},
				{
					label: '3세트',
					result: '승리',
					lane: 'SUPPORT',
					championId: 'Thresh',
					championName: '쓰레쉬',
					mmrBefore: 1481,
					mmrAfter: 1510,
					blueTeam: teamWithTarget(blueTeam, 'Thresh', '쓰레쉬', 'SUPPORT', 29),
					redTeam,
					blueBans: ['Ahri', 'LeeSin', 'Rakan'],
					redBans: ['Ornn', 'Kaisa', 'Viego']
				}
			]
		},
		{
			id: 'custom-5',
			result: '승리',
			type: '경매 내전',
			participantCount: 10,
			seriesName: '수요 경매 내전',
			score: '2 : 0',
			wins: 2,
			losses: 0,
			mmrBefore: 1556,
			mmrAfter: 1584,
			playedAt: '5일 전',
			sets: [
				{
					label: '1세트',
					result: '승리',
					lane: 'MID',
					championId: 'Syndra',
					championName: '신드라',
					mmrBefore: 1556,
					mmrAfter: 1570,
					blueTeam: teamWithTarget(blueTeam, 'Syndra', '신드라', 'MID', 14),
					redTeam,
					blueBans: ['Renekton', 'Vi', 'Kaisa'],
					redBans: ['Ornn', 'LeeSin', 'Ezreal']
				},
				{
					label: '2세트',
					result: '승리',
					lane: 'MID',
					championId: 'Azir',
					championName: '아지르',
					mmrBefore: 1570,
					mmrAfter: 1584,
					blueTeam: teamWithTarget(blueTeam, 'Azir', '아지르', 'MID', 14),
					redTeam,
					blueBans: ['Jax', 'Nocturne', 'Syndra'],
					redBans: ['KSante', 'Ahri', 'Rakan']
				}
			]
		}
	];

	const championRecords: ChampionRecord[] = [
		{ id: 'Ahri', name: '아리', wins: 8, losses: 4, mainLane: 'MID' },
		{ id: 'Orianna', name: '오리아나', wins: 5, losses: 3, mainLane: 'MID' },
		{ id: 'Ezreal', name: '이즈리얼', wins: 3, losses: 3, mainLane: 'BOTTOM' },
		{ id: 'Sylas', name: '사일러스', wins: 3, losses: 2, mainLane: 'MID' },
		{ id: 'Nautilus', name: '노틸러스', wins: 3, losses: 1, mainLane: 'SUPPORT' }
	];

	const auctionTournaments: AuctionTournamentRecord[] = [
		{
			id: 'auction-summer',
			name: '여름 경매 내전',
			teamName: '탐솔팀',
			spentPoints: 780,
			placement: '2위',
			playedAt: '6일 전',
			roster: blueTeam,
			series: matches.filter((match) => match.participantCount === 20).reverse(),
			opponents: { 'custom-4': '모카팀', 'custom-2': '서초팀' },
			stages: { 'custom-4': '4강', 'custom-2': '결승' }
		}
	];

	const filteredMatches = $derived(
		matches.filter(
			(match) =>
				match.participantCount === 10 &&
				(typeFilter === '전체' || typeFilter === match.type) &&
				(laneFilter === '전체' || match.sets.some((set) => set.lane === laneFilter))
		)
	);

	const filteredAuctionTournaments = $derived(
		auctionTournaments.filter(
			(tournament) =>
				(typeFilter === '전체' || typeFilter === '경매 내전') &&
				(laneFilter === '전체' ||
					tournament.series.some((series) => series.sets.some((set) => set.lane === laneFilter)))
		)
	);

	function championIconUrl(id: string): string {
		return `/dd/cdn/${dataDragonVersion}/img/champion/${id}.png`;
	}

	function signed(value: number): string {
		return `${value > 0 ? '+' : ''}${value}`;
	}

	function toggleSeries(match: CustomMatch) {
		if (expandedMatchId === match.id) {
			expandedMatchId = null;
			return;
		}

		expandedMatchId = match.id;
		expandedTournamentId = null;
		selectedSetIndex = match.sets.length - 1;
	}

	function toggleTournament(tournament: AuctionTournamentRecord) {
		if (expandedTournamentId === tournament.id) {
			expandedTournamentId = null;
			return;
		}

		const selectedSeries =
			tournament.series.find((series) => series.id === selectedAuctionSeriesId) ??
			tournament.series.at(-1);

		expandedTournamentId = tournament.id;
		expandedMatchId = null;
		selectedAuctionSeriesId = selectedSeries?.id ?? '';
		selectedSetIndex = Math.max(0, (selectedSeries?.sets.length ?? 1) - 1);
	}

	function selectAuctionSeries(series: CustomMatch) {
		selectedAuctionSeriesId = series.id;
		selectedSetIndex = series.sets.length - 1;
	}

	function tournamentMmrChange(tournament: AuctionTournamentRecord): number {
		return tournament.series.reduce(
			(total, series) => total + (series.mmrAfter - series.mmrBefore),
			0
		);
	}

	function tournamentSetRecord(tournament: AuctionTournamentRecord): {
		wins: number;
		losses: number;
	} {
		return tournament.series.reduce(
			(total, series) => ({ wins: total.wins + series.wins, losses: total.losses + series.losses }),
			{ wins: 0, losses: 0 }
		);
	}

	function tournamentSeriesRecord(tournament: AuctionTournamentRecord): {
		wins: number;
		losses: number;
	} {
		return tournament.series.reduce(
			(total, series) => ({
				wins: total.wins + (series.result === '승리' ? 1 : 0),
				losses: total.losses + (series.result === '패배' ? 1 : 0)
			}),
			{ wins: 0, losses: 0 }
		);
	}

	function activeAuctionSeries(tournament: AuctionTournamentRecord): CustomMatch {
		return (
			tournament.series.find((series) => series.id === selectedAuctionSeriesId) ??
			tournament.series[0]
		);
	}

	function submitRecordSearch(event: globalThis.SubmitEvent) {
		event.preventDefault();
		const query = recordSearch.trim();
		const target = resolve('/g/[guildId]/records', { guildId: data.guildId });
		goto(query ? `${target}?q=${globalThis.encodeURIComponent(query)}` : target);
	}
</script>

<svelte:head>
	<title>{searchedUserName} 내전 전적 | MonkeyBot</title>
</svelte:head>

<div class="custom-records-page">
	<main class="custom-records-shell">
		<form class="records-search" onsubmit={submitRecordSearch}>
			<div>
				<span>{guild.displayName}</span>
				<strong>내전 전적</strong>
			</div>
			<label>
				<Search size={15} aria-hidden="true" />
				<input
					bind:value={recordSearch}
					placeholder="Discord 사용자 또는 Riot ID"
					aria-label="길드 내전 전적 검색"
				/>
			</label>
			<button type="submit">조회</button>
		</form>

		<section class="profile-section">
			<img
				src={`/dd/cdn/${dataDragonVersion}/img/profileicon/588.png`}
				alt={`${searchedUserName} 프로필`}
			/>
			<div class="profile-copy">
				<p>{guild.displayName} · 내전 참가자</p>
				<h1>{searchedUserName}</h1>
				<span>{searchedUserTag}</span>
			</div>
			<label class="season-control">
				<span>시즌</span>
				<select bind:value={season} aria-label="내전 시즌">
					<option>시즌 3</option>
					<option>시즌 2</option>
					<option>전체 시즌</option>
				</select>
			</label>
		</section>

		<section class="lane-mmr-grid" aria-label="라인별 MMR">
			{#each laneMmr as lane (lane.lane)}
				<div class="lane-mmr-cell">
					<PositionIcon role={lane.lane} size="md" />
					<div>
						<span>{laneLabel[lane.lane]}</span>
						<strong>{lane.mmr}</strong>
					</div>
					<p>{lane.games}게임 <b class:negative={lane.change < 0}>{signed(lane.change)}</b></p>
				</div>
			{/each}
		</section>

		<section class="record-summary">
			<div><span>승패</span><strong>33승 21패</strong></div>
			<div><span>주 포지션</span><strong>미드</strong></div>
		</section>

		<section class="matches-section">
			<div class="filter-toolbar">
				<div class="filter-group">
					{#each ['전체', '일반 내전', '경매 내전'] as filter (filter)}
						<button
							type="button"
							class:active={typeFilter === filter}
							onclick={() => (typeFilter = filter as '전체' | MatchType)}>{filter}</button
						>
					{/each}
				</div>
				<div class="filter-group lane-filter">
					<button
						type="button"
						class:active={laneFilter === '전체'}
						onclick={() => (laneFilter = '전체')}>전체 라인</button
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
					{#each filteredMatches as match (match.id)}
						<div class="match-entry">
							<div
								class:loss={match.result === '패배'}
								class:auction={match.type === '경매 내전'}
								class="match-row"
							>
								<div class="result-cell">
									<strong>{match.type === '일반 내전' ? '일반' : '경매'}</strong>
									<span>{match.participantCount}인</span>
								</div>
								<div class="series-cell">
									<strong>{match.seriesName}</strong>
								</div>
								<div class="score-cell">
									<strong>{match.score}</strong>
								</div>
								<div class="mmr-cell series-mmr-cell">
									<strong class:negative={match.mmrAfter < match.mmrBefore}
										>MMR {signed(match.mmrAfter - match.mmrBefore)}</strong
									>
								</div>
								<div class="date-cell">{match.playedAt}</div>
								<button
									type="button"
									class="expand-button"
									aria-label={`${match.seriesName} 상세`}
									aria-expanded={expandedMatchId === match.id}
									onclick={() => toggleSeries(match)}
									>{expandedMatchId === match.id ? '⌃' : '⌄'}</button
								>
							</div>

							{#if expandedMatchId === match.id}
								{@const selectedSet = match.sets[selectedSetIndex] ?? match.sets[0]}
								<div class="expanded-match">
									<div class="set-selector">
										{#each match.sets as set, index (set.label)}
											<button
												type="button"
												class:active={selectedSetIndex === index}
												onclick={() => (selectedSetIndex = index)}
											>
												<span class:set-loss={set.result === '패배'}>{set.result}</span>
												<strong>{set.label}</strong>
												<img src={championIconUrl(set.championId)} alt="" />
												<PositionIcon role={set.lane} size="sm" />
												<small>MMR {signed(set.mmrAfter - set.mmrBefore)}</small>
											</button>
										{/each}
									</div>
									<div class="lineup-grid">
										{@render TeamLineup(
											'블루팀',
											selectedSet.blueTeam,
											selectedSet.result === '승리'
										)}
										{@render TeamLineup(
											'레드팀',
											selectedSet.redTeam,
											selectedSet.result === '패배'
										)}
									</div>
									<div class="pick-ban-row">
										<span>블루 밴</span>
										<div>
											{#each selectedSet.blueBans as champion (champion)}<img
													src={championIconUrl(champion)}
													alt=""
												/>{/each}
										</div>
										<span>레드 밴</span>
										<div>
											{#each selectedSet.redBans as champion (champion)}<img
													src={championIconUrl(champion)}
													alt=""
												/>{/each}
										</div>
									</div>
								</div>
							{/if}
						</div>
					{/each}

					{#each filteredAuctionTournaments as tournament (tournament.id)}
						{@const seriesRecord = tournamentSeriesRecord(tournament)}
						{@const setRecord = tournamentSetRecord(tournament)}
						{@const mmrChange = tournamentMmrChange(tournament)}
						<div class="auction-entry">
							<button
								type="button"
								class="auction-row"
								aria-label={`${tournament.name} 상세`}
								aria-expanded={expandedTournamentId === tournament.id}
								onclick={() => toggleTournament(tournament)}
							>
								<div class="auction-type-cell"><strong>경매</strong><span>20인</span></div>
								<div class="auction-event-cell">
									<strong>{tournament.name}</strong>
									<span>{tournament.teamName} · {tournament.spentPoints}P</span>
								</div>
								<div class="auction-score-cell">
									<strong>{tournament.placement}</strong>
									<span>시리즈 {seriesRecord.wins}승 {seriesRecord.losses}패</span>
								</div>
								<div class="auction-summary-cell">
									<span>세트 {setRecord.wins}승 {setRecord.losses}패</span>
									<strong class:negative={mmrChange < 0}>MMR {signed(mmrChange)}</strong>
								</div>
								<div class="date-cell">{tournament.playedAt}</div>
								<span class="expand-button" aria-hidden="true"
									>{expandedTournamentId === tournament.id ? '⌃' : '⌄'}</span
								>
							</button>

							{#if expandedTournamentId === tournament.id}
								{@const selectedSeries = activeAuctionSeries(tournament)}
								{@const selectedSet =
									selectedSeries.sets[selectedSetIndex] ?? selectedSeries.sets[0]}
								<div class="expanded-tournament">
									<div class="auction-team-summary">
										<div class="auction-roster">
											{#each tournament.roster as player (player.name)}
												<div class:target-player={player.isTarget}>
													<PositionIcon role={player.lane} size="sm" />
													<img src={championIconUrl(player.championId)} alt="" />
													<span>
														<strong>{player.name}</strong><small>{laneLabel[player.lane]}</small>
													</span>
												</div>
											{/each}
										</div>
									</div>

									<div class="auction-series-selector">
										{#each tournament.series as series (series.id)}
											<button
												type="button"
												class:active={selectedSeries.id === series.id}
												class:loss={series.result === '패배'}
												onclick={() => selectAuctionSeries(series)}
											>
												<span>{tournament.stages[series.id]}</span>
												<strong>vs {tournament.opponents[series.id]}</strong>
												<b>{series.score}</b>
												<small>MMR {signed(series.mmrAfter - series.mmrBefore)}</small>
											</button>
										{/each}
									</div>

									<div class="auction-set-detail">
										<div class="set-selector">
											{#each selectedSeries.sets as set, index (set.label)}
												<button
													type="button"
													class:active={selectedSetIndex === index}
													onclick={() => (selectedSetIndex = index)}
												>
													<span class:set-loss={set.result === '패배'}>{set.result}</span>
													<strong>{set.label}</strong>
													<img src={championIconUrl(set.championId)} alt="" />
													<PositionIcon role={set.lane} size="sm" />
													<small>MMR {signed(set.mmrAfter - set.mmrBefore)}</small>
												</button>
											{/each}
										</div>
										<div class="lineup-grid">
											{@render TeamLineup(
												'블루팀',
												selectedSet.blueTeam,
												selectedSet.result === '승리'
											)}
											{@render TeamLineup(
												'레드팀',
												selectedSet.redTeam,
												selectedSet.result === '패배'
											)}
										</div>
										<div class="pick-ban-row">
											<span>블루 밴</span>
											<div>
												{#each selectedSet.blueBans as champion (champion)}<img
														src={championIconUrl(champion)}
														alt=""
													/>{/each}
											</div>
											<span>레드 밴</span>
											<div>
												{#each selectedSet.redBans as champion (champion)}<img
														src={championIconUrl(champion)}
														alt=""
													/>{/each}
											</div>
										</div>
									</div>
								</div>
							{/if}
						</div>
					{/each}
				</div>
			</div>
		</section>

		<section class="champion-records">
			<header>
				<h2>챔피언 사용 기록</h2>
				<span>{season}</span>
			</header>
			<div class="champion-table-scroll">
				<div class="champion-table">
					<div class="champion-table-row champion-table-head">
						<span>챔피언</span><span>전적</span><span>주 포지션</span>
					</div>
					{#each championRecords as champion (champion.id)}
						<div class="champion-table-row">
							<span><img src={championIconUrl(champion.id)} alt="" />{champion.name}</span>
							<span>{champion.wins}승 {champion.losses}패</span>
							<span
								><PositionIcon role={champion.mainLane} size="sm" />{laneLabel[
									champion.mainLane
								]}</span
							>
						</div>
					{/each}
				</div>
			</div>
		</section>
	</main>
</div>

{#snippet TeamLineup(label: string, players: LineupPlayer[], winner: boolean)}
	<section class:winner class="team-lineup">
		<header>
			<h3>{label}</h3>
			{#if winner}<span>승리</span>{/if}
		</header>
		<div>
			{#each players as player (player.name)}
				<div class:target-player={player.isTarget} class="lineup-player">
					<PositionIcon role={player.lane} size="sm" />
					<img src={championIconUrl(player.championId)} alt={player.championName} />
					<div><strong>{player.name}</strong><span>{player.championName}</span></div>
				</div>
			{/each}
		</div>
	</section>
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
	}
	.records-search {
		display: grid;
		grid-template-columns: minmax(150px, 0.35fr) minmax(260px, 1fr) 72px;
		align-items: center;
		gap: 8px;
		margin-bottom: 8px;
		padding: 10px 12px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}
	.records-search > div {
		display: grid;
		gap: 2px;
	}
	.records-search span {
		color: #64748b;
		font-size: 10px;
		font-weight: 700;
	}
	.records-search strong {
		color: #f8fafc;
		font-size: 14px;
	}
	.records-search label {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 38px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-control);
		padding: 0 11px;
		color: #64748b;
	}
	.records-search input {
		min-width: 0;
		width: 100%;
		border: 0;
		background: transparent;
		color: #f8fafc;
		font-size: 13px;
		outline: 0;
	}
	.records-search button {
		height: 38px;
		border: 1px solid rgb(34 211 238 / 0.35);
		background: rgb(34 211 238 / 0.08);
		color: #cffafe;
		font-size: 12px;
		font-weight: 700;
	}
	.profile-section {
		display: grid;
		grid-template-columns: 96px minmax(0, 1fr) auto;
		align-items: center;
		gap: 16px;
		padding: 14px;
		background: var(--color-app-surface);
	}
	.profile-section > img {
		width: 96px;
		height: 96px;
		object-fit: cover;
	}
	.profile-copy {
		min-width: 0;
	}
	.profile-copy p,
	.profile-copy span {
		margin: 0;
		color: #94a3b8;
		font-size: 12px;
		font-weight: 600;
	}
	.profile-copy h1 {
		margin: 5px 0;
		color: #f8fafc;
		font-size: 27px;
		line-height: 1.1;
	}
	.season-control {
		display: grid;
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
	@media (max-width: 820px) {
		.custom-records-page {
			padding: 0 0 40px;
		}
		.custom-records-shell {
			padding: 0 8px 8px;
		}
		.records-search {
			grid-template-columns: 1fr 72px;
		}
		.records-search > div {
			grid-column: 1 / -1;
		}
		.profile-section {
			grid-template-columns: 72px minmax(0, 1fr);
		}
		.profile-section > img {
			width: 72px;
			height: 72px;
		}
		.season-control {
			grid-column: 1 / -1;
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
