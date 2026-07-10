export const LANES = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;
export const TEAMS = ['TEAM_1', 'TEAM_2'] as const;

export type Lane = (typeof LANES)[number];
export type Team = (typeof TEAMS)[number];
export type EntrySlot = `${Team}_${Lane}`;
export type SynergyKind = 'TOP_JUNGLE' | 'JUNGLE_MID' | 'BOTTOM_SUPPORT';

export const laneLabel: Record<Lane, string> = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
};

export const teamLabel: Record<Team, string> = {
	TEAM_1: '1팀',
	TEAM_2: '2팀'
};

export interface MockEntryParticipant {
	id: string;
	name: string;
	roles: Lane[];
	mainRole: Lane;
	mmr: number;
	recentRecord: string;
	riotTier?: string | null;
	avatarUrl?: string | null;
	profileIconId?: number | null;
}

export interface MockAuctionMember {
	id: string;
	name: string;
	role: Lane;
	mmr: number;
	price: number;
	acquiredVia?: 'CAPTAIN' | 'BID' | 'MANUAL';
	acquiredAtPoints?: number | null;
	riotTier?: string | null;
	recentRecord?: string | null;
	winRate?: number | null;
	games?: number | null;
	mainChampions?: string[];
	avatarUrl?: string | null;
	profileIconId?: number | null;
}

export interface MockEntrySynergyRecord {
	team: Team;
	kind: SynergyKind;
	userAId: string;
	userBId: string;
	plays: number;
	wins: number;
	losses: number;
}

export interface MockEntryHeadToHeadRecord {
	lane: Lane;
	userAId: string;
	userBId: string;
	plays: number;
	userAWins: number;
	userBWins: number;
}

export interface MockAuctionTeam {
	id: string;
	name: string;
	captain: string;
	initialPoints?: number;
	points: number;
	members: MockAuctionMember[];
}

export const mockEntryParticipants: MockEntryParticipant[] = [
	{
		id: 'u-top-1',
		name: '서초 탑솔',
		roles: ['TOP'],
		mainRole: 'TOP',
		mmr: 1620,
		recentRecord: '최근 5승 3패',
		riotTier: 'E2',
		profileIconId: 588
	},
	{
		id: 'u-jgl-1',
		name: '한강 정글',
		roles: ['JUNGLE', 'MID'],
		mainRole: 'JUNGLE',
		mmr: 1580,
		recentRecord: '최근 6승 2패',
		riotTier: 'P1',
		profileIconId: 907
	},
	{
		id: 'u-mid-1',
		name: '홍대 미드',
		roles: ['MID'],
		mainRole: 'MID',
		mmr: 1710,
		recentRecord: '최근 7승 1패',
		riotTier: 'D4',
		profileIconId: 1113
	},
	{
		id: 'u-bot-1',
		name: '강남 원딜',
		roles: ['BOTTOM'],
		mainRole: 'BOTTOM',
		mmr: 1490,
		recentRecord: '최근 4승 4패',
		riotTier: 'P3',
		profileIconId: 1381
	},
	{
		id: 'u-sup-1',
		name: '잠실 서폿',
		roles: ['SUPPORT', 'JUNGLE'],
		mainRole: 'SUPPORT',
		mmr: 1510,
		recentRecord: '최근 5승 3패',
		riotTier: 'P2',
		profileIconId: 1454
	},
	{
		id: 'u-top-2',
		name: '마포 탑',
		roles: ['TOP', 'MID'],
		mainRole: 'TOP',
		mmr: 1475,
		recentRecord: '최근 3승 5패',
		riotTier: 'P4',
		profileIconId: 1625
	},
	{
		id: 'u-jgl-2',
		name: '성수 정글',
		roles: ['JUNGLE'],
		mainRole: 'JUNGLE',
		mmr: 1660,
		recentRecord: '최근 6승 2패',
		riotTier: 'E1',
		profileIconId: 1631
	},
	{
		id: 'u-mid-2',
		name: '용산 미드',
		roles: ['MID'],
		mainRole: 'MID',
		mmr: 1535,
		recentRecord: '최근 4승 4패',
		riotTier: 'P2',
		profileIconId: 2090
	},
	{
		id: 'u-bot-2',
		name: '노원 원딜',
		roles: ['BOTTOM', 'SUPPORT'],
		mainRole: 'BOTTOM',
		mmr: 1605,
		recentRecord: '최근 6승 2패',
		riotTier: 'E4',
		profileIconId: 3157
	},
	{
		id: 'u-sup-2',
		name: '분당 서폿',
		roles: ['SUPPORT'],
		mainRole: 'SUPPORT',
		mmr: 1450,
		recentRecord: '최근 3승 5패',
		riotTier: 'G1',
		profileIconId: 3379
	}
];

export const mockEntryAssignments: Record<string, EntrySlot> = {
	'u-top-1': 'TEAM_1_TOP',
	'u-top-2': 'TEAM_2_TOP',
	'u-jgl-1': 'TEAM_1_JUNGLE',
	'u-mid-1': 'TEAM_1_MID',
	'u-mid-2': 'TEAM_2_MID',
	'u-bot-2': 'TEAM_2_BOTTOM'
};

export const mockEntrySynergies: MockEntrySynergyRecord[] = [
	{
		team: 'TEAM_1',
		kind: 'TOP_JUNGLE',
		userAId: 'u-top-1',
		userBId: 'u-jgl-1',
		plays: 7,
		wins: 5,
		losses: 2
	},
	{
		team: 'TEAM_1',
		kind: 'JUNGLE_MID',
		userAId: 'u-jgl-1',
		userBId: 'u-mid-1',
		plays: 4,
		wins: 1,
		losses: 3
	},
	{
		team: 'TEAM_2',
		kind: 'BOTTOM_SUPPORT',
		userAId: 'u-bot-2',
		userBId: 'u-sup-2',
		plays: 6,
		wins: 4,
		losses: 2
	}
];

export const mockEntryHeadToHead: MockEntryHeadToHeadRecord[] = [
	{
		lane: 'TOP',
		userAId: 'u-top-1',
		userBId: 'u-top-2',
		plays: 8,
		userAWins: 5,
		userBWins: 3
	},
	{
		lane: 'MID',
		userAId: 'u-mid-1',
		userBId: 'u-mid-2',
		plays: 6,
		userAWins: 2,
		userBWins: 4
	},
	{
		lane: 'BOTTOM',
		userAId: 'u-bot-1',
		userBId: 'u-bot-2',
		plays: 5,
		userAWins: 3,
		userBWins: 2
	}
];

export const mockAuctionTeams: MockAuctionTeam[] = [
	{
		id: 'auction-team-a',
		name: '1팀',
		captain: '서초 탑솔',
		initialPoints: 1000,
		points: 1000,
		members: [
			{
				id: 'captain-a',
				name: '서초 탑솔',
				role: 'TOP',
				mmr: 1620,
				price: 0,
				acquiredVia: 'CAPTAIN',
				acquiredAtPoints: null,
				riotTier: 'E2',
				profileIconId: 588
			}
		]
	},
	{
		id: 'auction-team-b',
		name: '2팀',
		captain: '홍대 미드',
		initialPoints: 1000,
		points: 1000,
		members: [
			{
				id: 'captain-b',
				name: '홍대 미드',
				role: 'MID',
				mmr: 1710,
				price: 0,
				acquiredVia: 'CAPTAIN',
				acquiredAtPoints: null,
				riotTier: 'D4',
				profileIconId: 1113
			}
		]
	},
	{
		id: 'auction-team-c',
		name: '3팀',
		captain: '성수 정글',
		initialPoints: 1000,
		points: 1000,
		members: [
			{
				id: 'captain-c',
				name: '성수 정글',
				role: 'JUNGLE',
				mmr: 1660,
				price: 0,
				acquiredVia: 'CAPTAIN',
				acquiredAtPoints: null,
				riotTier: 'E1',
				profileIconId: 1631
			}
		]
	},
	{
		id: 'auction-team-d',
		name: '4팀',
		captain: '노원 원딜',
		initialPoints: 1000,
		points: 1000,
		members: [
			{
				id: 'captain-d',
				name: '노원 원딜',
				role: 'BOTTOM',
				mmr: 1605,
				price: 0,
				acquiredVia: 'CAPTAIN',
				acquiredAtPoints: null,
				riotTier: 'E4',
				profileIconId: 3157
			}
		]
	}
];

export const mockAuctionQueue: MockAuctionMember[] = [
	{
		id: 'bid-1',
		name: '한강 정글',
		role: 'JUNGLE',
		mmr: 1580,
		price: 180,
		riotTier: 'P1',
		recentRecord: '최근 6승 2패',
		winRate: 64,
		games: 42,
		mainChampions: ['리 신', '바이', '자르반 4세'],
		profileIconId: 907
	},
	{
		id: 'bid-2',
		name: '강남 원딜',
		role: 'BOTTOM',
		mmr: 1490,
		price: 120,
		riotTier: 'P3',
		recentRecord: '최근 4승 4패',
		winRate: 52,
		games: 37,
		mainChampions: ['카이사', '이즈리얼', '자야'],
		profileIconId: 1381
	},
	{
		id: 'bid-3',
		name: '잠실 서폿',
		role: 'SUPPORT',
		mmr: 1510,
		price: 90,
		riotTier: 'P2',
		recentRecord: '최근 5승 3패',
		winRate: 58,
		games: 31,
		mainChampions: ['라칸', '노틸러스', '룰루'],
		profileIconId: 1454
	},
	{
		id: 'bid-4',
		name: '분당 서폿',
		role: 'SUPPORT',
		mmr: 1450,
		price: 80,
		riotTier: 'G1',
		recentRecord: '최근 3승 5패',
		winRate: 49,
		games: 29,
		mainChampions: ['브라움', '밀리오', '레오나'],
		profileIconId: 3379
	},
	{
		id: 'bid-5',
		name: '마포 탑',
		role: 'TOP',
		mmr: 1475,
		price: 110,
		riotTier: 'P4',
		recentRecord: '최근 3승 5패',
		winRate: 51,
		games: 34,
		mainChampions: ['크산테', '레넥톤', '나르'],
		profileIconId: 1625
	},
	{
		id: 'bid-6',
		name: '용산 미드',
		role: 'MID',
		mmr: 1535,
		price: 135,
		riotTier: 'P2',
		recentRecord: '최근 4승 4패',
		winRate: 55,
		games: 40,
		mainChampions: ['아리', '오리아나', '탈리야'],
		profileIconId: 2090
	},
	{
		id: 'bid-7',
		name: '왕십리 정글',
		role: 'JUNGLE',
		mmr: 1420,
		price: 70,
		riotTier: 'G2',
		recentRecord: '최근 2승 6패',
		winRate: 47,
		games: 25,
		mainChampions: ['세주아니', '마오카이', '렐'],
		profileIconId: 4022
	},
	{
		id: 'bid-8',
		name: '합정 원딜',
		role: 'BOTTOM',
		mmr: 1685,
		price: 240,
		riotTier: 'D4',
		recentRecord: '최근 7승 1패',
		winRate: 69,
		games: 46,
		mainChampions: ['아펠리오스', '징크스', '제리'],
		profileIconId: 4568
	},
	{
		id: 'bid-9',
		name: '상암 서폿',
		role: 'SUPPORT',
		mmr: 1570,
		price: 145,
		riotTier: 'E4',
		recentRecord: '최근 6승 2패',
		winRate: 61,
		games: 39,
		mainChampions: ['쓰레쉬', '라칸', '알리스타'],
		profileIconId: 5080
	},
	{
		id: 'bid-10',
		name: '광교 탑',
		role: 'TOP',
		mmr: 1640,
		price: 210,
		riotTier: 'E2',
		recentRecord: '최근 5승 3패',
		winRate: 60,
		games: 33,
		mainChampions: ['잭스', '피오라', '카밀'],
		profileIconId: 5217
	},
	{
		id: 'bid-11',
		name: '문래 미드',
		role: 'MID',
		mmr: 1460,
		price: 95,
		riotTier: 'P4',
		recentRecord: '최근 4승 4패',
		winRate: 50,
		games: 28,
		mainChampions: ['벡스', '갈리오', '리산드라'],
		profileIconId: 5398
	},
	{
		id: 'bid-12',
		name: '일산 정글',
		role: 'JUNGLE',
		mmr: 1505,
		price: 125,
		riotTier: 'P3',
		recentRecord: '최근 5승 3패',
		winRate: 56,
		games: 36,
		mainChampions: ['녹턴', '킨드레드', '그레이브즈'],
		profileIconId: 5467
	},
	{
		id: 'bid-13',
		name: '수원 원딜',
		role: 'BOTTOM',
		mmr: 1555,
		price: 150,
		riotTier: 'P1',
		recentRecord: '최근 6승 2패',
		winRate: 59,
		games: 44,
		mainChampions: ['루시안', '칼리스타', '애쉬'],
		profileIconId: 5634
	},
	{
		id: 'bid-14',
		name: '송파 서폿',
		role: 'SUPPORT',
		mmr: 1395,
		price: 60,
		riotTier: 'G3',
		recentRecord: '최근 2승 6패',
		winRate: 44,
		games: 22,
		mainChampions: ['소라카', '잔나', '나미'],
		profileIconId: 5880
	},
	{
		id: 'bid-15',
		name: '압구정 탑',
		role: 'TOP',
		mmr: 1725,
		price: 280,
		riotTier: 'D3',
		recentRecord: '최근 7승 1패',
		winRate: 71,
		games: 48,
		mainChampions: ['럼블', '아트록스', '그웬'],
		profileIconId: 5884
	},
	{
		id: 'bid-16',
		name: '구로 미드',
		role: 'MID',
		mmr: 1600,
		price: 190,
		riotTier: 'E3',
		recentRecord: '최근 5승 3패',
		winRate: 57,
		games: 41,
		mainChampions: ['요네', '사일러스', '아칼리'],
		profileIconId: 5893
	}
];

export const mockAuctionCaptainIds = mockAuctionTeams
	.map((team) => team.members[0]?.id)
	.filter(Boolean);

export const mockAuctionRecruitmentMembers: MockAuctionMember[] = [
	...mockAuctionTeams.flatMap((team) => team.members.map((member) => ({ ...member }))),
	...mockAuctionQueue.map((member) => ({ ...member, price: 0 }))
];

export const mockAuctionBracketTeams: MockAuctionTeam[] = mockAuctionTeams.map((team, index) => {
	const additions = mockAuctionQueue.slice(index * 4, index * 4 + 4).map((member) => ({
		...member,
		acquiredVia: 'BID' as const,
		acquiredAtPoints: member.price
	}));
	const spent = additions.reduce((sum, member) => sum + member.price, 0);

	return {
		...team,
		points: team.points - spent,
		members: [...team.members.map((member) => ({ ...member })), ...additions]
	};
});
