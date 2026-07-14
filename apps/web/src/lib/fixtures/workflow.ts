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

export const mockEntryParticipants: MockEntryParticipant[] = [
	{
		id: 'u-top-1',
		name: '서초 탑솔',
		roles: ['TOP'],
		mainRole: 'TOP',
		mmr: 1620,
		recentRecord: '최근 5승 3패',
		riotTier: 'E2',
		mainChampions: ['Gnar', 'Aatrox', 'Renekton', 'Poppy', 'Yone'],
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
		mainChampions: ['LeeSin', 'Viego', 'Vi', 'Graves', 'Sejuani'],
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
		mainChampions: ['Ahri', 'Orianna', 'Syndra', 'Azir', 'Yone'],
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
		mainChampions: ['Ezreal', 'Kaisa', 'Jinx', 'Varus', 'Ashe'],
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
		mainChampions: ['Thresh', 'Rakan', 'Nautilus', 'Alistar', 'Blitzcrank'],
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
		mainChampions: ['Poppy', 'Gnar', 'Aatrox', 'Renekton', 'Yone'],
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
		mainChampions: ['Vi', 'Sejuani', 'Viego', 'LeeSin', 'Graves'],
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
		mainChampions: ['Syndra', 'Azir', 'Orianna', 'Ahri', 'Yone'],
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
		mainChampions: ['Kaisa', 'Ashe', 'Jinx', 'Ezreal', 'Varus'],
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
		mainChampions: ['Alistar', 'Nautilus', 'Rakan', 'Thresh', 'Blitzcrank'],
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

export const mockCompletedEntryAssignments: Record<string, EntrySlot> = {
	'u-top-1': 'TEAM_1_TOP',
	'u-jgl-1': 'TEAM_1_JUNGLE',
	'u-mid-1': 'TEAM_1_MID',
	'u-bot-1': 'TEAM_1_BOTTOM',
	'u-sup-1': 'TEAM_1_SUPPORT',
	'u-top-2': 'TEAM_2_TOP',
	'u-jgl-2': 'TEAM_2_JUNGLE',
	'u-mid-2': 'TEAM_2_MID',
	'u-bot-2': 'TEAM_2_BOTTOM',
	'u-sup-2': 'TEAM_2_SUPPORT'
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
