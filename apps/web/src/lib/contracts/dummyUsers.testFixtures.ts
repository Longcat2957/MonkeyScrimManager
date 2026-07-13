import type { DummyGroup, DummyUser } from './dummyUsers';

export const dummyUserFixture: DummyUser = {
	userId: 'dmy_01900000-0000-7000-8000-000000000001',
	identityKind: 'DUMMY',
	stableKey: 'mid-player-01',
	displayName: '테스트 미드 01',
	avatarSeed: 'mid-01',
	notes: null,
	state: 'ACTIVE',
	revision: 3,
	privacy: {
		publicProfileEnabled: true,
		showRiotIds: false,
		allowExternalRecordSearch: false
	},
	riotAccount: null,
	championPreferences: {
		TOP: [],
		JUNGLE: [],
		MID: ['Ahri'],
		BOTTOM: [],
		SUPPORT: []
	},
	guildMemberships: [
		{
			guildId: '1525851181157580861',
			guildName: 'MonkeyBot Test',
			displayName: '테스트 미드 01',
			status: 'ACTIVE',
			mainLane: 'MID',
			laneMmr: { top: 1500, jungle: 1500, mid: 1580, bottom: 1500, support: 1500 }
		}
	],
	groupIds: ['01900000-0000-7000-8000-000000000010'],
	createdAt: '2026-07-13T03:00:00Z',
	updatedAt: '2026-07-13T03:05:00Z'
};

export const dummyGroupFixture: DummyGroup = {
	id: '01900000-0000-7000-8000-000000000010',
	name: '20인 경매 풀',
	description: null,
	revision: 2,
	memberCount: 1,
	members: [
		{
			userId: dummyUserFixture.userId,
			displayName: dummyUserFixture.displayName,
			position: 1
		}
	],
	createdAt: '2026-07-13T03:00:00Z',
	updatedAt: '2026-07-13T03:05:00Z'
};
