export type ScrimPickBanResultInputSide = 'BLUE' | 'RED';
export type ScrimPickBanResultInputSidePreset = 'DEFAULT' | 'SWAPPED';
export type ScrimPickBanResultInputLane = 'TOP' | 'JUNGLE' | 'MID' | 'BOTTOM' | 'SUPPORT';

export interface ScrimPickBanResultInputChampion {
	id: string;
	name: string;
	imageFull: string;
}

export interface ScrimPickBanResultInputPlayer {
	id: string;
	name: string;
	lane: ScrimPickBanResultInputLane;
	mmr: number;
	riotTier?: string | null;
	preferredChampions: string[];
	frequentChampions: string[];
}

export interface ScrimPickBanResultInputTeam {
	side: ScrimPickBanResultInputSide;
	name: string;
	players: ScrimPickBanResultInputPlayer[];
}

export interface ScrimPickBanResultInputGame {
	id: string;
	label: string;
	sidePreset?: ScrimPickBanResultInputSidePreset;
	sideSelected?: boolean;
	winner: ScrimPickBanResultInputSide | null;
	bans: Record<ScrimPickBanResultInputSide, Array<string | null>>;
	picks: Record<string, string | null>;
}

export const scrimPickBanResultInputLaneLabel: Record<ScrimPickBanResultInputLane, string> = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
};

export const scrimPickBanResultInputChampions: ScrimPickBanResultInputChampion[] = [
	{ id: 'Aatrox', name: '아트록스', imageFull: 'Aatrox.png' },
	{ id: 'Ahri', name: '아리', imageFull: 'Ahri.png' },
	{ id: 'Alistar', name: '알리스타', imageFull: 'Alistar.png' },
	{ id: 'Ashe', name: '애쉬', imageFull: 'Ashe.png' },
	{ id: 'Azir', name: '아지르', imageFull: 'Azir.png' },
	{ id: 'Blitzcrank', name: '블리츠크랭크', imageFull: 'Blitzcrank.png' },
	{ id: 'Caitlyn', name: '케이틀린', imageFull: 'Caitlyn.png' },
	{ id: 'Ezreal', name: '이즈리얼', imageFull: 'Ezreal.png' },
	{ id: 'Gnar', name: '나르', imageFull: 'Gnar.png' },
	{ id: 'Graves', name: '그레이브즈', imageFull: 'Graves.png' },
	{ id: 'Jinx', name: '징크스', imageFull: 'Jinx.png' },
	{ id: 'Kaisa', name: '카이사', imageFull: 'Kaisa.png' },
	{ id: 'LeeSin', name: '리 신', imageFull: 'LeeSin.png' },
	{ id: 'Nautilus', name: '노틸러스', imageFull: 'Nautilus.png' },
	{ id: 'Orianna', name: '오리아나', imageFull: 'Orianna.png' },
	{ id: 'Poppy', name: '뽀삐', imageFull: 'Poppy.png' },
	{ id: 'Rakan', name: '라칸', imageFull: 'Rakan.png' },
	{ id: 'Renekton', name: '레넥톤', imageFull: 'Renekton.png' },
	{ id: 'Sejuani', name: '세주아니', imageFull: 'Sejuani.png' },
	{ id: 'Syndra', name: '신드라', imageFull: 'Syndra.png' },
	{ id: 'Thresh', name: '쓰레쉬', imageFull: 'Thresh.png' },
	{ id: 'Varus', name: '바루스', imageFull: 'Varus.png' },
	{ id: 'Vi', name: '바이', imageFull: 'Vi.png' },
	{ id: 'Viego', name: '비에고', imageFull: 'Viego.png' },
	{ id: 'Yone', name: '요네', imageFull: 'Yone.png' }
];

export const mockScrimPickBanResultInputTeams: Record<
	ScrimPickBanResultInputSide,
	ScrimPickBanResultInputTeam
> = {
	BLUE: {
		side: 'BLUE',
		name: '1팀',
		players: [
			{
				id: 'blue-top',
				name: '도현',
				lane: 'TOP',
				mmr: 1518,
				riotTier: 'P3',
				preferredChampions: ['Aatrox', 'Gnar', 'Poppy'],
				frequentChampions: ['Gnar', 'Aatrox', 'Renekton']
			},
			{
				id: 'blue-jungle',
				name: '서초',
				lane: 'JUNGLE',
				mmr: 1595,
				riotTier: 'E2',
				preferredChampions: ['LeeSin', 'Viego', 'Vi'],
				frequentChampions: ['LeeSin', 'Viego', 'Graves']
			},
			{
				id: 'blue-mid',
				name: '탐솔',
				lane: 'MID',
				mmr: 1642,
				riotTier: 'D4',
				preferredChampions: ['Ahri', 'Orianna', 'Yone'],
				frequentChampions: ['Ahri', 'Orianna', 'Syndra']
			},
			{
				id: 'blue-bottom',
				name: '마루',
				lane: 'BOTTOM',
				mmr: 1588,
				riotTier: 'E4',
				preferredChampions: ['Ezreal', 'Kaisa', 'Varus'],
				frequentChampions: ['Ezreal', 'Kaisa', 'Jinx']
			},
			{
				id: 'blue-support',
				name: '하루',
				lane: 'SUPPORT',
				mmr: 1510,
				riotTier: 'P2',
				preferredChampions: ['Thresh', 'Rakan', 'Blitzcrank'],
				frequentChampions: ['Thresh', 'Rakan', 'Nautilus']
			}
		]
	},
	RED: {
		side: 'RED',
		name: '2팀',
		players: [
			{
				id: 'red-top',
				name: '준서',
				lane: 'TOP',
				mmr: 1532,
				riotTier: 'P2',
				preferredChampions: ['Poppy', 'Gnar', 'Renekton'],
				frequentChampions: ['Poppy', 'Aatrox', 'Gnar']
			},
			{
				id: 'red-jungle',
				name: '민기',
				lane: 'JUNGLE',
				mmr: 1560,
				riotTier: 'P1',
				preferredChampions: ['Vi', 'Sejuani', 'Viego'],
				frequentChampions: ['Vi', 'Sejuani', 'Viego']
			},
			{
				id: 'red-mid',
				name: '로아',
				lane: 'MID',
				mmr: 1611,
				riotTier: 'E3',
				preferredChampions: ['Syndra', 'Azir', 'Orianna'],
				frequentChampions: ['Syndra', 'Azir', 'Yone']
			},
			{
				id: 'red-bottom',
				name: '이든',
				lane: 'BOTTOM',
				mmr: 1575,
				riotTier: 'E4',
				preferredChampions: ['Kaisa', 'Ashe', 'Jinx'],
				frequentChampions: ['Kaisa', 'Varus', 'Ashe']
			},
			{
				id: 'red-support',
				name: '유림',
				lane: 'SUPPORT',
				mmr: 1528,
				riotTier: 'P3',
				preferredChampions: ['Alistar', 'Nautilus', 'Rakan'],
				frequentChampions: ['Alistar', 'Blitzcrank', 'Nautilus']
			}
		]
	}
};

function pickMap(values: Array<[string, string | null]>): Record<string, string | null> {
	return Object.fromEntries(values);
}

export const mockScrimPickBanResultInputGames: ScrimPickBanResultInputGame[] = [
	{
		id: 'game-1',
		label: '1세트',
		winner: 'BLUE',
		bans: {
			BLUE: ['Renekton', 'Sejuani', 'Azir', 'Caitlyn', 'Rakan'],
			RED: ['Aatrox', 'LeeSin', 'Orianna', 'Jinx', 'Nautilus']
		},
		picks: pickMap([
			['blue-top', 'Gnar'],
			['blue-jungle', 'Viego'],
			['blue-mid', 'Ahri'],
			['blue-bottom', 'Ezreal'],
			['blue-support', 'Thresh'],
			['red-top', 'Poppy'],
			['red-jungle', 'Vi'],
			['red-mid', 'Syndra'],
			['red-bottom', 'Kaisa'],
			['red-support', 'Alistar']
		])
	},
	{
		id: 'game-2',
		label: '2세트',
		winner: null,
		bans: {
			BLUE: ['Aatrox', 'Sejuani', null, null, null],
			RED: ['Ahri', 'LeeSin', null, null, null]
		},
		picks: pickMap([
			['blue-top', 'Renekton'],
			['blue-jungle', 'Graves'],
			['blue-mid', null],
			['blue-bottom', null],
			['blue-support', null],
			['red-top', 'Gnar'],
			['red-jungle', 'Viego'],
			['red-mid', null],
			['red-bottom', null],
			['red-support', null]
		])
	},
	{
		id: 'game-3',
		label: '3세트',
		winner: null,
		bans: {
			BLUE: [null, null, null, null, null],
			RED: [null, null, null, null, null]
		},
		picks: pickMap([
			['blue-top', null],
			['blue-jungle', null],
			['blue-mid', null],
			['blue-bottom', null],
			['blue-support', null],
			['red-top', null],
			['red-jungle', null],
			['red-mid', null],
			['red-bottom', null],
			['red-support', null]
		])
	}
];

export const mockScrimPickBanResultInputPatch = '16.13.1';
