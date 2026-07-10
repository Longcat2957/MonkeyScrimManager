export type QuickRecordLane = '탑' | '정글' | '미드' | '원딜' | '서폿';

export interface QuickRecordChampion {
	id: string;
	name: string;
	imageFull: string;
	games: number;
	wins: number;
	losses: number;
}

export interface QuickRecordRecentGame {
	id: string;
	label: string;
	champion: QuickRecordChampion;
	lane: QuickRecordLane;
	result: '승' | '패';
	playedAt: string;
}

export interface QuickRecordSearchResult {
	id: string;
	displayName: string;
	riotId: string;
	guildName: string;
	dataDragonVersion: string;
	profileIconId: number;
	custom: {
		games: number;
		wins: number;
		losses: number;
		winRate: number;
		mainLane: QuickRecordLane;
		laneMmr: Array<{ lane: QuickRecordLane; mmr: number; games: number }>;
		champions: QuickRecordChampion[];
		recentGames: QuickRecordRecentGame[];
	};
	riot: {
		tier: string;
		rankType: string;
		games: number;
		wins: number;
		losses: number;
		winRate: number;
		recentChampions: QuickRecordChampion[];
		lifetimeChampions: QuickRecordChampion[];
		refreshedAt: string;
	};
}

function champion(
	id: string,
	name: string,
	imageFull: string,
	games: number,
	wins: number
): QuickRecordChampion {
	return {
		id,
		name,
		imageFull,
		games,
		wins,
		losses: games - wins
	};
}

const ahri = champion('Ahri', '아리', 'Ahri.png', 11, 7);
const orianna = champion('Orianna', '오리아나', 'Orianna.png', 8, 5);
const sylas = champion('Sylas', '사일러스', 'Sylas.png', 6, 3);
const yone = champion('Yone', '요네', 'Yone.png', 5, 4);
const viktor = champion('Viktor', '빅토르', 'Viktor.png', 4, 2);
const syndra = champion('Syndra', '신드라', 'Syndra.png', 4, 3);
const leesin = champion('LeeSin', '리 신', 'LeeSin.png', 13, 8);
const viego = champion('Viego', '비에고', 'Viego.png', 7, 4);
const graves = champion('Graves', '그레이브즈', 'Graves.png', 6, 4);
const nidalee = champion('Nidalee', '니달리', 'Nidalee.png', 5, 2);
const sejuani = champion('Sejuani', '세주아니', 'Sejuani.png', 4, 3);
const kindred = champion('Kindred', '킨드레드', 'Kindred.png', 4, 2);
const ezreal = champion('Ezreal', '이즈리얼', 'Ezreal.png', 10, 6);
const jinx = champion('Jinx', '징크스', 'Jinx.png', 7, 4);
const kaisa = champion('Kaisa', '카이사', 'Kaisa.png', 7, 5);
const thresh = champion('Thresh', '쓰레쉬', 'Thresh.png', 5, 3);
const nautilus = champion('Nautilus', '노틸러스', 'Nautilus.png', 4, 2);

function buildRecentGames(
	prefix: string,
	picks: Array<{ champion: QuickRecordChampion; lane: QuickRecordLane; result: '승' | '패' }>
): QuickRecordRecentGame[] {
	return Array.from({ length: 20 }, (_, index) => {
		const pick = picks[index % picks.length];
		const gameNumber = 52 - index;

		return {
			id: `${prefix}-${index + 1}`,
			label: index % 5 === 0 ? `경매 스크림 ${gameNumber}` : `정기 내전 ${gameNumber}`,
			champion: pick.champion,
			lane: pick.lane,
			result: pick.result,
			playedAt: index === 0 ? '오늘' : `${index + 1}일 전`
		};
	});
}

export const mockQuickRecordResults: QuickRecordSearchResult[] = [
	{
		id: 'mock-record-tamsol',
		displayName: '탐솔',
		riotId: '탐솔#KR1',
		guildName: 'MSM Seoul',
		dataDragonVersion: '16.13.1',
		profileIconId: 588,
		custom: {
			games: 54,
			wins: 33,
			losses: 21,
			winRate: 61,
			mainLane: '미드',
			laneMmr: [
				{ lane: '탑', mmr: 1518, games: 4 },
				{ lane: '정글', mmr: 1540, games: 3 },
				{ lane: '미드', mmr: 1642, games: 31 },
				{ lane: '원딜', mmr: 1588, games: 10 },
				{ lane: '서폿', mmr: 1510, games: 6 }
			],
			champions: [ahri, orianna, sylas, yone, viktor, syndra],
			recentGames: buildRecentGames('tamsol-custom', [
				{ champion: ahri, lane: '미드', result: '승' },
				{ champion: orianna, lane: '미드', result: '패' },
				{ champion: sylas, lane: '미드', result: '승' },
				{ champion: yone, lane: '미드', result: '승' },
				{ champion: ezreal, lane: '원딜', result: '패' },
				{ champion: thresh, lane: '서폿', result: '승' }
			])
		},
		riot: {
			tier: 'D4',
			rankType: '솔로랭크',
			games: 48,
			wins: 28,
			losses: 20,
			winRate: 58,
			recentChampions: [ahri, orianna, yone, sylas, viktor, syndra],
			lifetimeChampions: [ahri, orianna, sylas, yone, ezreal, viktor],
			refreshedAt: '방금 갱신'
		}
	},
	{
		id: 'mock-record-jungle',
		displayName: '서초',
		riotId: '서초#KR2',
		guildName: 'MSM Seoul',
		dataDragonVersion: '16.13.1',
		profileIconId: 539,
		custom: {
			games: 47,
			wins: 25,
			losses: 22,
			winRate: 53,
			mainLane: '정글',
			laneMmr: [
				{ lane: '탑', mmr: 1508, games: 5 },
				{ lane: '정글', mmr: 1595, games: 30 },
				{ lane: '미드', mmr: 1484, games: 3 },
				{ lane: '원딜', mmr: 1460, games: 4 },
				{ lane: '서폿', mmr: 1518, games: 5 }
			],
			champions: [leesin, viego, graves, nidalee, sejuani, kindred],
			recentGames: buildRecentGames('seocho-custom', [
				{ champion: leesin, lane: '정글', result: '승' },
				{ champion: viego, lane: '정글', result: '패' },
				{ champion: graves, lane: '정글', result: '승' },
				{ champion: nidalee, lane: '정글', result: '패' },
				{ champion: sejuani, lane: '정글', result: '승' },
				{ champion: nautilus, lane: '서폿', result: '패' }
			])
		},
		riot: {
			tier: 'E2',
			rankType: '솔로랭크',
			games: 55,
			wins: 30,
			losses: 25,
			winRate: 55,
			recentChampions: [leesin, viego, graves, nidalee, sejuani, kindred],
			lifetimeChampions: [leesin, viego, graves, nidalee, jinx, kaisa],
			refreshedAt: '방금 갱신'
		}
	}
];

export function searchMockQuickRecord(query: string): QuickRecordSearchResult | null {
	const token = query.trim().toLowerCase();

	if (!token) {
		return null;
	}

	return (
		mockQuickRecordResults.find((record) =>
			[
				record.displayName,
				record.riotId,
				record.guildName,
				record.custom.mainLane,
				...record.custom.champions.map((championItem) => championItem.name),
				...record.riot.recentChampions.map((championItem) => championItem.name),
				...record.riot.lifetimeChampions.map((championItem) => championItem.name)
			]
				.join(' ')
				.toLowerCase()
				.includes(token)
		) ?? null
	);
}
