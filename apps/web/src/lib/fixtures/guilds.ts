export interface MockGuildSummary {
	id: string;
	name: string;
	shortName: string;
	memberCount: number;
	role: '운영자' | '참가자';
	activity: string;
}

export const DEFAULT_GUILD_ID = 'guild-msm';
export const DEFAULT_GUILD_COOKIE = 'monkeybot_default_guild';

export const mockGuilds: MockGuildSummary[] = [
	{
		id: DEFAULT_GUILD_ID,
		name: 'MSM Seoul',
		shortName: 'MSM',
		memberCount: 84,
		role: '운영자',
		activity: '4개 작업 진행 중'
	},
	{
		id: 'guild-weekend',
		name: '주말 내전방',
		shortName: '주말',
		memberCount: 42,
		role: '운영자',
		activity: '1개 작업 진행 중'
	},
	{
		id: 'guild-academy',
		name: 'Monkey Academy',
		shortName: 'MA',
		memberCount: 116,
		role: '참가자',
		activity: '최근 내전 2일 전'
	}
];

export function getMockGuild(guildId: string): MockGuildSummary {
	return mockGuilds.find((guild) => guild.id === guildId) ?? mockGuilds[0]!;
}

export function isMockGuildId(guildId: string): boolean {
	return mockGuilds.some((guild) => guild.id === guildId);
}
