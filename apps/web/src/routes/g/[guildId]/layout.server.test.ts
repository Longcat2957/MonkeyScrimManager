import { describe, expect, it, vi } from 'vitest';
import { load } from './+layout.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const userId = '100000000000000001';

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('guild layout server load', () => {
	it('accepts a real Discord snowflake and supplies the shell from API context', async () => {
		const guild = {
			id: guildId,
			displayName: 'MSM Seoul',
			iconUrl: null,
			memberCount: 84,
			currentUserRole: 'OPERATOR',
			isExternallyPublic: false,
			currentSeason: null
		};
		const user = {
			discordId: userId,
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: guildId,
			serviceAdmin: false
		};
		const summary = {
			id: guildId,
			displayName: 'MSM Seoul',
			iconUrl: null,
			memberCount: 84,
			currentUserRole: 'OPERATOR'
		};
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === `/api/guilds/${guildId}`) return envelope(guild);
			if (path === '/api/me/guilds') return envelope({ guilds: [summary] });
			if (path === '/api/me') return envelope(user);
			throw new Error(`unexpected request: ${path}`);
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId },
			url: new URL(`http://localhost:5173/g/${guildId}`)
		} as never);
		if (!result) throw new Error('guild layout load returned no data');

		expect(fetcher.mock.calls.map(([input]) => String(input))).toEqual([
			`/api/guilds/${guildId}`,
			'/api/me',
			'/api/me/guilds'
		]);
		expect(result).toMatchObject({ guildId, guild, user, guilds: [summary] });
	});
});
