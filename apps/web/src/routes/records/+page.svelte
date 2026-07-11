<script lang="ts">
	interface ChampionRow {
		id: string;
		name: string;
		games: number;
		winRate: string;
		kda: string;
		kills: string;
		deaths: string;
		assists: string;
		cs: string;
		gold: string;
		wins: number;
		losses: number;
	}

	interface MatchRow {
		championId: string;
		champion: string;
		level: number;
		result: '승리' | '패배';
		lp: string;
		kda: string;
		ratio: string;
		spells: string;
		cs: string;
		gold: string;
		duration: string;
		playedAt: string;
		spellsIds: string[];
		itemIds: string[];
	}

	interface ExpandedPlayer {
		tier: string;
		championId: string;
		name: string;
		tag: string;
		level: number;
		kda: string;
		kdaRatio: string;
		killRate: string;
		cs: string;
		gold: string;
		damage: string;
		damageTaken: string;
		vision: string;
		itemIds: string[];
		spellIds: string[];
	}

	const dataDragonVersion = '16.13.1';
	let currentQueue = $state('전체');
	let previousQueue = $state('전체');
	let matchQueue = $state('전체');
	let refreshed = $state(false);
	let currentExpanded = $state(false);
	let previousExpanded = $state(false);
	let expandedMatch = $state<string | null>('36:21');
	let detailTab = $state<'summary' | 'build'>('summary');

	const seasons = [
		'S15 PLATINUM II',
		'S14-3 GOLD II',
		'S14-2 PLATINUM II',
		'S14-1 EMERALD IV',
		'S13-2 GOLD III'
	];

	const rankQueues = [
		{ queue: '솔로랭크', tier: 'PLATINUM III', record: '23 LP · 22승 29패' },
		{ queue: '자유랭크', tier: 'UNRANKED', record: '최근 기록 없음' }
	];

	const championHeaders = [
		'챔피언',
		'게임',
		'승률',
		'KDA',
		'킬',
		'데스',
		'어시',
		'CS',
		'골드',
		'트리플',
		'쿼드라',
		'펜타',
		'승 / 패'
	];

	const currentSeason: ChampionRow[] = [
		{
			id: 'Naafiri',
			name: '나피리',
			games: 10,
			winRate: '30.0%',
			kda: '2.96',
			kills: '7.2',
			deaths: '5.0',
			assists: '7.6',
			cs: '211',
			gold: '14,048',
			wins: 3,
			losses: 7
		},
		{
			id: 'Lillia',
			name: '릴리아',
			games: 7,
			winRate: '42.9%',
			kda: '3.70',
			kills: '4.9',
			deaths: '2.9',
			assists: '5.7',
			cs: '194',
			gold: '11,937',
			wins: 3,
			losses: 4
		},
		{
			id: 'Ekko',
			name: '에코',
			games: 5,
			winRate: '40.0%',
			kda: '2.21',
			kills: '5.8',
			deaths: '5.8',
			assists: '7.0',
			cs: '160',
			gold: '11,838',
			wins: 2,
			losses: 3
		},
		{
			id: 'XinZhao',
			name: '신 짜오',
			games: 5,
			winRate: '0.0%',
			kda: '1.60',
			kills: '3.2',
			deaths: '5.0',
			assists: '4.8',
			cs: '157',
			gold: '9,409',
			wins: 0,
			losses: 5
		},
		{
			id: 'LeeSin',
			name: '리 신',
			games: 4,
			winRate: '100%',
			kda: '5.36',
			kills: '6.2',
			deaths: '2.8',
			assists: '8.5',
			cs: '159',
			gold: '10,937',
			wins: 4,
			losses: 0
		}
	];

	const previousSeason: ChampionRow[] = [
		{
			id: 'Viego',
			name: '비에고',
			games: 8,
			winRate: '50.0%',
			kda: '3.74',
			kills: '8.5',
			deaths: '4.2',
			assists: '7.4',
			cs: '180',
			gold: '11,955',
			wins: 4,
			losses: 4
		},
		{
			id: 'Karthus',
			name: '카서스',
			games: 7,
			winRate: '100%',
			kda: '4.67',
			kills: '10.9',
			deaths: '4.3',
			assists: '9.1',
			cs: '173',
			gold: '11,168',
			wins: 7,
			losses: 0
		},
		{
			id: 'Xerath',
			name: '제라스',
			games: 7,
			winRate: '57.1%',
			kda: '5.71',
			kills: '9.7',
			deaths: '3.4',
			assists: '9.9',
			cs: '228',
			gold: '14,283',
			wins: 4,
			losses: 3
		},
		{
			id: 'Graves',
			name: '그레이브즈',
			games: 7,
			winRate: '57.1%',
			kda: '2.49',
			kills: '6.7',
			deaths: '5.9',
			assists: '7.8',
			cs: '193',
			gold: '12,511',
			wins: 4,
			losses: 3
		},
		{
			id: 'TwistedFate',
			name: '트위스티드 페이트',
			games: 4,
			winRate: '75.0%',
			kda: '3.13',
			kills: '3.8',
			deaths: '4.8',
			assists: '11.3',
			cs: '205',
			gold: '12,045',
			wins: 3,
			losses: 1
		}
	];

	const matches: MatchRow[] = [
		{
			championId: 'Naafiri',
			champion: '나피리',
			level: 17,
			result: '승리',
			lp: '+23 LP',
			kda: '5 / 4 / 11',
			ratio: '4.00',
			spells: '점멸 · 강타',
			cs: 'CS 229',
			gold: '14,607 G',
			duration: '36:21',
			playedAt: '19일 전',
			spellsIds: ['SummonerFlash', 'SummonerSmite'],
			itemIds: ['6692', '3047', '3071', '3053', '6333', '3364']
		},
		{
			championId: 'Vi',
			champion: '바이',
			level: 15,
			result: '패배',
			lp: '-17 LP',
			kda: '2 / 6 / 11',
			ratio: '2.17',
			spells: '점멸 · 강타',
			cs: 'CS 172',
			gold: '11,912 G',
			duration: '34:23',
			playedAt: '2개월 전',
			spellsIds: ['SummonerFlash', 'SummonerSmite'],
			itemIds: ['6632', '3047', '3071', '3053', '1036', '3364']
		},
		{
			championId: 'LeeSin',
			champion: '리 신',
			level: 13,
			result: '승리',
			lp: '승급',
			kda: '8 / 4 / 9',
			ratio: '4.25',
			spells: '점멸 · 강타',
			cs: 'CS 129',
			gold: '10,602 G',
			duration: '26:30',
			playedAt: '2개월 전',
			spellsIds: ['SummonerFlash', 'SummonerSmite'],
			itemIds: ['6632', '3111', '3071', '6333', '3053', '3364']
		},
		{
			championId: 'Maokai',
			champion: '마오카이',
			level: 12,
			result: '패배',
			lp: '-18 LP',
			kda: '7 / 8 / 9',
			ratio: '2.00',
			spells: '점멸 · 강타',
			cs: 'CS 28',
			gold: '9,409 G',
			duration: '27:29',
			playedAt: '2개월 전',
			spellsIds: ['SummonerFlash', 'SummonerSmite'],
			itemIds: ['6662', '3047', '3065', '3075', '3109', '3364']
		},
		{
			championId: 'Lillia',
			champion: '릴리아',
			level: 16,
			result: '승리',
			lp: '+23 LP',
			kda: '5 / 6 / 12',
			ratio: '2.83',
			spells: '점멸 · 강타',
			cs: 'CS 188',
			gold: '13,072 G',
			duration: '33:49',
			playedAt: '2개월 전',
			spellsIds: ['SummonerFlash', 'SummonerSmite'],
			itemIds: ['6653', '3020', '3116', '4637', '3135', '3364']
		}
	];

	const blueTeam: ExpandedPlayer[] = [
		{
			tier: 'P3',
			championId: 'Gwen',
			name: '코끼리막국수',
			tag: '#KR1',
			level: 20,
			kda: '5 / 6 / 9',
			kdaRatio: '2.33',
			killRate: '35%',
			cs: 'CS 253 (7)',
			gold: '골드 14,189',
			damage: '31,679',
			damageTaken: '39,010',
			vision: '34 / 3',
			itemIds: ['1054', '3115', '4633', '3089', '3135', '3364'],
			spellIds: ['SummonerFlash', 'SummonerTeleport']
		},
		{
			tier: 'P3',
			championId: 'LeeSin',
			name: 'Longcat',
			tag: '#KR1',
			level: 17,
			kda: '5 / 4 / 11',
			kdaRatio: '4.00',
			killRate: '40%',
			cs: 'CS 229 (6.3)',
			gold: '골드 14,607',
			damage: '14,019',
			damageTaken: '28,559',
			vision: '44 / 10',
			itemIds: ['6692', '3047', '3071', '3053', '6333', '3364'],
			spellIds: ['SummonerFlash', 'SummonerSmite']
		},
		{
			tier: 'P2',
			championId: 'Zed',
			name: '배준휘',
			tag: '#전라도',
			level: 18,
			kda: '17 / 4 / 4',
			kdaRatio: '5.25',
			killRate: '53%',
			cs: 'CS 277 (7.6)',
			gold: '골드 19,946',
			damage: '41,477',
			damageTaken: '28,476',
			vision: '26 / 0',
			itemIds: ['6692', '3142', '3814', '3071', '3156', '3364'],
			spellIds: ['SummonerFlash', 'SummonerDot']
		},
		{
			tier: 'P4',
			championId: 'Ziggs',
			name: '닭잡기좋은날',
			tag: '#KR1',
			level: 18,
			kda: '8 / 3 / 11',
			kdaRatio: '6.33',
			killRate: '48%',
			cs: 'CS 326 (9)',
			gold: '골드 18,242',
			damage: '33,714',
			damageTaken: '16,553',
			vision: '21 / 1',
			itemIds: ['6655', '3020', '3089', '3135', '4645', '3364'],
			spellIds: ['SummonerFlash', 'SummonerTeleport']
		},
		{
			tier: 'P2',
			championId: 'Swain',
			name: '원징징괴물서폿그리고...',
			tag: '',
			level: 15,
			kda: '5 / 8 / 18',
			kdaRatio: '2.88',
			killRate: '58%',
			cs: 'CS 57 (1.6)',
			gold: '골드 12,860',
			damage: '20,475',
			damageTaken: '43,784',
			vision: '109 / 11',
			itemIds: ['3865', '3116', '3157', '3065', '3089', '3364'],
			spellIds: ['SummonerFlash', 'SummonerDot']
		}
	];

	const redTeam: ExpandedPlayer[] = [
		{
			tier: 'P4',
			championId: 'Cassiopeia',
			name: '해왕의 파수꾼',
			tag: '#KR1',
			level: 20,
			kda: '3 / 8 / 6',
			kdaRatio: '1.12',
			killRate: '36%',
			cs: 'CS 267 (7.3)',
			gold: '골드 12,281',
			damage: '28,786',
			damageTaken: '46,240',
			vision: '34 / 4',
			itemIds: ['6657', '3070', '3116', '3157', '3089', '3364'],
			spellIds: ['SummonerFlash', 'SummonerTeleport']
		},
		{
			tier: 'P2',
			championId: 'Nidalee',
			name: '영동지킴이',
			tag: '#KR1',
			level: 17,
			kda: '6 / 5 / 7',
			kdaRatio: '2.60',
			killRate: '52%',
			cs: 'CS 231 (6.4)',
			gold: '골드 14,556',
			damage: '20,830',
			damageTaken: '40,565',
			vision: '38 / 4',
			itemIds: ['3100', '3020', '3102', '3089', '4645', '3364'],
			spellIds: ['SummonerFlash', 'SummonerSmite']
		},
		{
			tier: 'G1',
			championId: 'Malzahar',
			name: '오우우우우우',
			tag: '#KR1',
			level: 16,
			kda: '1 / 9 / 7',
			kdaRatio: '0.89',
			killRate: '32%',
			cs: 'CS 230 (6.3)',
			gold: '골드 11,385',
			damage: '15,363',
			damageTaken: '24,041',
			vision: '29 / 0',
			itemIds: ['6653', '3020', '3116', '3165', '3089', '3364'],
			spellIds: ['SummonerFlash', 'SummonerTeleport']
		},
		{
			tier: 'P4',
			championId: 'Janna',
			name: '손현우 따까리',
			tag: '#KR1',
			level: 16,
			kda: '7 / 8 / 6',
			kdaRatio: '1.62',
			killRate: '52%',
			cs: 'CS 346 (9.5)',
			gold: '골드 19,897',
			damage: '29,718',
			damageTaken: '27,837',
			vision: '43 / 0',
			itemIds: ['6671', '3006', '3031', '3094', '3072', '3363'],
			spellIds: ['SummonerFlash', 'SummonerDot']
		},
		{
			tier: 'P3',
			championId: 'Veigar',
			name: 'GunsN Roses',
			tag: '#9014',
			level: 15,
			kda: '8 / 10 / 9',
			kdaRatio: '1.70',
			killRate: '68%',
			cs: 'CS 81 (2.2)',
			gold: '골드 12,807',
			damage: '26,155',
			damageTaken: '25,936',
			vision: '105 / 18',
			itemIds: ['3871', '6655', '3089', '3135', '4645', '3364'],
			spellIds: ['SummonerFlash', 'SummonerDot']
		}
	];

	function championCells(champion: ChampionRow): string[] {
		return [
			champion.name,
			String(champion.games),
			champion.winRate,
			champion.kda,
			champion.kills,
			champion.deaths,
			champion.assists,
			champion.cs,
			champion.gold,
			'0',
			'0',
			'0',
			`${champion.wins} / ${champion.losses}`
		];
	}

	function championIconUrl(id: string): string {
		return `/dd/cdn/${dataDragonVersion}/img/champion/${id}.png`;
	}

	function itemIconUrl(id: string): string {
		return `/dd/cdn/${dataDragonVersion}/img/item/${id}.png`;
	}

	function spellIconUrl(id: string): string {
		return `/dd/cdn/${dataDragonVersion}/img/spell/${id}.png`;
	}
</script>

<svelte:head>
	<title>전적 | MonkeyBot</title>
</svelte:head>

<div class="records-page">
	<main class="records-shell" aria-label="전적 페이지 레이아웃">
		<section class="profile-layout">
			<div class="profile-avatar">
				<img
					src={`/dd/cdn/${dataDragonVersion}/img/profileicon/6668.png`}
					alt="Longcat#KR1 프로필"
				/>
			</div>
			<div class="profile-copy">
				<p class="profile-meta-line">KR · LEVEL 258</p>
				<h1 class="profile-name-line">Longcat<span>#KR1</span></h1>
				<button type="button" class="profile-action-line" onclick={() => (refreshed = true)}>
					{refreshed ? '방금 갱신됨' : '전적 갱신'}
				</button>
			</div>
			<div class="season-history">
				{#each seasons as season (season)}
					<span>{season}</span>
				{/each}
			</div>
		</section>

		<section class="rank-grid">
			{#each rankQueues as rank, index (rank.queue)}
				<div class="rank-panel">
					<div class="rank-mark">
						{#if index === 0}
							<img src="/riot/ranks/platinum.png" alt="플래티넘" />
						{:else}
							-
						{/if}
					</div>
					<div class="rank-copy">
						<span>{rank.queue}</span>
						<strong>{rank.tier}</strong>
						<span>{rank.record}</span>
					</div>
				</div>
			{/each}
		</section>

		<section class="table-section">
			<div class="filter-row">
				{#each ['전체', '솔로랭크', '자유랭크'] as filter (filter)}
					<button
						type="button"
						class:active={currentQueue === filter}
						onclick={() => (currentQueue = filter)}>{filter}</button
					>
				{/each}
			</div>
			<div class="table-scroll">
				<div class="data-table">
					<div class="data-row data-head">
						{#each championHeaders as header (header)}<span>{header}</span>{/each}
					</div>
					{#each currentSeason.slice(0, currentExpanded ? currentSeason.length : 3) as champion (champion.name)}
						<div class="data-row">
							{#each championCells(champion) as cell, index}
								<span>
									{#if index === 0}
										<img src={championIconUrl(champion.id)} alt="" class="table-champion-icon" />
									{/if}
									{cell}
								</span>
							{/each}
						</div>
					{/each}
				</div>
			</div>
			<button type="button" class="table-more" onclick={() => (currentExpanded = !currentExpanded)}>
				{currentExpanded ? '접기' : '더 보기'}
			</button>
		</section>

		<section class="table-section">
			<div class="filter-row">
				{#each ['전체', '솔로랭크', '자유랭크'] as filter (filter)}
					<button
						type="button"
						class:active={previousQueue === filter}
						onclick={() => (previousQueue = filter)}>{filter}</button
					>
				{/each}
			</div>
			<div class="table-scroll">
				<div class="data-table">
					<div class="data-row data-head">
						{#each championHeaders as header (header)}<span>{header}</span>{/each}
					</div>
					{#each previousSeason.slice(0, previousExpanded ? previousSeason.length : 3) as champion (champion.name)}
						<div class="data-row">
							{#each championCells(champion) as cell, index}
								<span>
									{#if index === 0}
										<img src={championIconUrl(champion.id)} alt="" class="table-champion-icon" />
									{/if}
									{cell}
								</span>
							{/each}
						</div>
					{/each}
				</div>
			</div>
			<button
				type="button"
				class="table-more"
				onclick={() => (previousExpanded = !previousExpanded)}
			>
				{previousExpanded ? '접기' : '더 보기'}
			</button>
		</section>

		<section class="match-layout">
			<div class="match-filters">
				{#each ['전체', '솔로랭크', '자유랭크', '일반'] as filter (filter)}
					<button
						type="button"
						class:active={matchQueue === filter}
						onclick={() => (matchQueue = filter)}>{filter}</button
					>
				{/each}
			</div>

			<div class="summary-grid">
				<div class="summary-block"><strong>53.3%</strong><span>16승 14패</span></div>
				<div class="summary-block"><strong>3.53</strong><span>평균 KDA</span></div>
				<div class="summary-wide">
					{#each currentSeason.slice(0, 3) as champion (champion.name)}
						<div>
							<img src={championIconUrl(champion.id)} alt="" />
							<p>
								<strong>{champion.name}</strong><span
									>{champion.winRate} · {champion.games}게임</span
								>
							</p>
						</div>
					{/each}
				</div>
			</div>

			<div class="match-list-scroll">
				<div class="match-list">
					{#each matches as match (match.champion + match.duration)}
						<div class="match-entry">
							<div class:loss-row={match.result === '패배'} class="match-row">
								<div class="match-champion">
									<img src={championIconUrl(match.championId)} alt={match.champion} />
									<span>{match.level}</span>
								</div>
								<span><b>{match.result}</b><small>{match.lp}</small></span>
								<span><b>{match.ratio} KDA</b><small>{match.kda}</small></span>
								<span>
									<span class="spell-icons">
										{#each match.spellsIds as spell (spell)}<img
												src={spellIconUrl(spell)}
												alt=""
											/>{/each}
									</span>
									<small>{match.spells}</small>
								</span>
								<span class="item-icons">
									{#each match.itemIds as item (item)}<img src={itemIconUrl(item)} alt="" />{/each}
								</span>
								<span
									><b>{match.cs} · {match.gold}</b><small>{match.duration} · {match.playedAt}</small
									></span
								>
								<button
									type="button"
									class="match-toggle"
									aria-label={`${match.champion} 경기 상세`}
									aria-expanded={expandedMatch === match.duration}
									onclick={() =>
										(expandedMatch = expandedMatch === match.duration ? null : match.duration)}
								>
									{expandedMatch === match.duration ? '⌃' : '⌄'}
								</button>
							</div>

							{#if expandedMatch === match.duration}
								<section class="expanded-match" aria-label="36분 21초 경기 상세">
									<div class="detail-tabs">
										<button
											type="button"
											class:active={detailTab === 'summary'}
											onclick={() => (detailTab = 'summary')}>종합</button
										>
										<button
											type="button"
											class:active={detailTab === 'build'}
											onclick={() => (detailTab = 'build')}>빌드</button
										>
									</div>

									{#each blueTeam as player (player.name)}
										{@render ExpandedPlayerRow(player, 'blue')}
									{/each}
									<div class="team-summary blue-summary">
										<strong>승리팀</strong><span>40 / 25 / 53</span><span
											>포탑 11 · 드래곤 3 · 바론 1</span
										>
									</div>
									<div class="team-summary red-summary">
										<strong>패배팀</strong><span>25 / 40 / 35</span><span
											>포탑 3 · 드래곤 2 · 바론 0</span
										>
									</div>
									{#each redTeam as player (player.name)}
										{@render ExpandedPlayerRow(player, 'red')}
									{/each}
								</section>
							{/if}
						</div>
					{/each}
				</div>
			</div>
		</section>
	</main>
</div>

{#snippet ExpandedPlayerRow(player: ExpandedPlayer, side: 'blue' | 'red')}
	<div
		class:expanded-blue={side === 'blue'}
		class:expanded-red={side === 'red'}
		class="expanded-player-row"
	>
		<strong class="expanded-tier">{player.tier}</strong>
		<div class="expanded-identity">
			<img src={championIconUrl(player.championId)} alt={player.name} />
			<p><strong>{player.name}</strong><span>{player.tag}</span></p>
		</div>
		<div class="expanded-spells">
			{#each player.spellIds as spell (spell)}<img src={spellIconUrl(spell)} alt="" />{/each}
		</div>
		<strong class="expanded-level">{player.level}</strong>
		<div class="expanded-kda"><strong>{player.kdaRatio}</strong><span>{player.kda}</span></div>
		<span class="expanded-kill-rate">{player.killRate}</span>
		<div class="expanded-items">
			{#each player.itemIds as item (item)}<img src={itemIconUrl(item)} alt="" />{/each}
		</div>
		<div class="expanded-farm"><strong>{player.cs}</strong><span>{player.gold}</span></div>
		<div class="expanded-meter">
			<strong>{player.damage}</strong><span
				><i style={`width: ${Math.min(100, Number(player.damage.replace(',', '')) / 450)}%`}
				></i></span
			>
		</div>
		<div class="expanded-meter taken">
			<strong>{player.damageTaken}</strong><span
				><i style={`width: ${Math.min(100, Number(player.damageTaken.replace(',', '')) / 500)}%`}
				></i></span
			>
		</div>
		<div class="expanded-vision"><strong>{player.vision}</strong><span>시야</span></div>
	</div>
{/snippet}

<style>
	.records-page {
		min-height: calc(100vh - 57px);
		padding: 28px 16px 64px;
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
		grid-row: 1;
		display: grid;
		place-items: center;
		width: 112px;
		aspect-ratio: 1;
		background: var(--color-app-surface-muted);
		color: #67e8f9;
		font-size: 28px;
		font-weight: 800;
		overflow: hidden;
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

	.profile-meta-line {
		margin: 0;
		color: #94a3b8;
		font-size: 12px;
		font-weight: 700;
	}

	.profile-name-line {
		margin: 0;
		color: #f8fafc;
		font-size: 28px;
		line-height: 1.1;
	}

	.profile-name-line span {
		margin-left: 2px;
		color: #64748b;
		font-weight: 600;
	}

	.profile-action-line {
		height: 30px;
		padding: 0 14px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		color: #cbd5e1;
		font-size: 12px;
		font-weight: 700;
		cursor: pointer;
	}

	.profile-action-line:hover {
		border-color: rgb(34 211 238 / 0.45);
		color: #cffafe;
	}

	.season-history {
		grid-column: 1 / -1;
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}

	.season-history span {
		padding: 3px 7px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-control);
		color: #94a3b8;
		font-size: 10px;
		font-weight: 700;
	}

	.filter-row,
	.match-filters {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
	}

	.filter-row button,
	.match-filters button,
	.table-more {
		display: block;
		height: 30px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-control);
		color: #94a3b8;
		font-size: 13px;
		font-weight: 700;
		cursor: pointer;
	}

	.filter-row button.active,
	.match-filters button.active {
		border-color: rgb(34 211 238 / 0.45);
		background: rgb(34 211 238 / 0.1);
		color: #cffafe;
	}

	.rank-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 12px;
	}

	.rank-panel {
		display: grid;
		grid-template-columns: 132px minmax(0, 1fr);
		align-items: center;
		min-height: 128px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}

	.rank-mark {
		display: grid;
		place-items: center;
		width: 88px;
		aspect-ratio: 1;
		margin: auto;
		background: var(--color-app-surface-muted);
		color: #67e8f9;
		font-size: 22px;
		font-weight: 800;
	}

	.rank-mark img {
		display: block;
		width: 88px;
		height: 88px;
		object-fit: contain;
	}

	.rank-copy {
		display: grid;
		gap: 9px;
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

	.table-section {
		margin-top: 16px;
	}

	.filter-row {
		height: 44px;
	}

	.filter-row button {
		width: 92px;
		height: 28px;
	}

	.table-scroll,
	.match-list-scroll {
		overflow-x: auto;
	}

	.data-table {
		min-width: 920px;
		border-top: 1px solid var(--color-app-border);
		border-left: 1px solid var(--color-app-border);
	}

	.data-row {
		display: grid;
		grid-template-columns: 160px repeat(12, minmax(0, 1fr));
		min-height: 38px;
	}

	.data-row span {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 5px;
		border-right: 1px solid var(--color-app-border-muted);
		border-bottom: 1px solid var(--color-app-border-muted);
		background: #111a26;
		color: #94a3b8;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}

	.data-row span:first-child {
		justify-content: flex-start;
		gap: 7px;
		padding-left: 10px;
		color: #e2e8f0;
		font-weight: 700;
	}

	.data-head {
		min-height: 40px;
	}

	.data-head span {
		background: #1a2b35;
		color: #cbd5e1;
		font-weight: 700;
	}

	.table-champion-icon {
		display: block;
		width: 28px;
		height: 28px;
		flex: 0 0 28px;
		object-fit: cover;
	}

	.table-more {
		width: 88px;
		margin: 8px 0 0 auto;
	}

	.match-layout {
		margin-top: 22px;
	}

	.match-filters {
		justify-content: flex-start;
		min-height: 46px;
		border-bottom: 1px solid var(--color-app-border);
	}

	.match-filters button {
		width: 74px;
		height: 28px;
	}

	.summary-grid {
		display: grid;
		grid-template-columns: 180px 180px minmax(0, 1fr);
		min-height: 120px;
		margin-top: 10px;
		border: 1px solid var(--color-app-border);
		background: var(--color-app-surface);
	}

	.summary-block {
		display: grid;
		align-content: center;
		gap: 12px;
		padding: 18px;
		border-right: 1px solid var(--color-app-border);
	}

	.summary-block strong {
		color: #f8fafc;
		font-size: 28px;
		line-height: 1;
	}

	.summary-block span {
		color: #67e8f9;
		font-size: 12px;
		font-weight: 700;
	}

	.summary-wide {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		align-items: center;
		gap: 12px;
		padding: 18px;
	}

	.summary-wide > div {
		display: grid;
		grid-template-columns: 42px minmax(0, 1fr);
		align-content: center;
		align-items: center;
		gap: 5px;
		height: 62px;
		padding: 10px;
		background: var(--color-app-surface-muted);
	}

	.summary-wide img {
		display: block;
		width: 42px;
		height: 42px;
		object-fit: cover;
	}

	.summary-wide p {
		display: grid;
		gap: 3px;
		min-width: 0;
		margin: 0;
	}

	.summary-wide strong {
		color: #e2e8f0;
		font-size: 13px;
	}

	.summary-wide span {
		color: #64748b;
		font-size: 11px;
	}

	.match-list {
		min-width: 980px;
		margin-top: 10px;
	}

	.match-row {
		display: grid;
		grid-template-columns: 64px 92px 104px 164px 190px minmax(0, 1fr) 32px;
		min-height: 98px;
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
		font-size: 13px;
		font-variant-numeric: tabular-nums;
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
		bottom: 18px;
		min-width: 16px;
		padding: 1px 3px;
		background: rgb(0 0 0 / 0.78);
		color: #f8fafc;
		font-size: 9px;
		text-align: center;
	}

	.match-row b {
		color: #f8fafc;
		font-size: 12px;
	}

	.match-row small {
		color: #94a3b8;
		font-size: 11px;
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
		width: 22px;
		height: 22px;
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
		padding: 0 !important;
		border: 0;
		background: transparent;
		color: #64748b !important;
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

	.detail-tabs {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		height: 46px;
		border-bottom: 1px solid var(--color-app-border);
	}

	.detail-tabs button {
		height: 30px;
		padding: 0 16px;
		border: 1px solid var(--color-app-border);
		border-radius: 2px;
		background: var(--color-app-control);
		color: #94a3b8;
		font-size: 12px;
		font-weight: 700;
		cursor: pointer;
	}

	.detail-tabs button.active {
		border-color: #64748b;
		background: #e2e8f0;
		color: #111827;
	}

	.expanded-player-row {
		display: grid;
		grid-template-columns:
			48px minmax(150px, 1.2fr) 48px 42px 82px 50px minmax(160px, 1.4fr)
			104px 92px 92px 82px;
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

	.expanded-tier {
		justify-self: center;
		border-radius: 3px;
		background: #2c8c78;
		color: #ecfeff;
		font-size: 11px;
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
		font-size: 13px;
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
		grid-template-columns: repeat(6, 28px);
		gap: 2px;
	}

	.expanded-items img {
		width: 28px;
		height: 28px;
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

	@media (max-width: 720px) {
		.records-page {
			padding: 12px 0 40px;
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

		.rank-grid {
			grid-template-columns: 1fr;
		}

		.summary-grid {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}

		.summary-wide {
			grid-column: 1 / -1;
			border-top: 1px solid var(--color-app-border);
		}
	}
</style>
