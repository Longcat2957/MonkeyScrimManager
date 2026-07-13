import { describe, expect, it, vi } from 'vitest';
import { dummyGroupFixture, dummyUserFixture } from '$lib/contracts/dummyUsers.testFixtures';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000099';

describe('/dev/users server load', () => {
	it('loads users, guild catalog and groups as separate authoritative resources', async () => {
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			const data =
				path === '/api/me'
					? {
							discordId: '1525847677542207548',
							displayName: 'Service Admin',
							avatarUrl: null,
							defaultGuildId: '1525851181157580861',
							serviceAdmin: true
						}
					: path === '/api/me/guilds'
						? {
								guilds: [
									{
										id: '1525851181157580861',
										displayName: 'MonkeyBot Test',
										iconUrl: null,
										memberCount: 24,
										currentUserRole: 'OPERATOR'
									}
								]
							}
						: path.endsWith('/catalog')
							? {
									guilds: [
										{
											guildId: '1525851181157580861',
											displayName: 'MonkeyBot Test',
											status: 'ACTIVE',
											activeSeasonId: null
										}
									]
								}
							: path.endsWith('/dummy-groups')
								? { groups: [dummyGroupFixture] }
								: { users: [dummyUserFixture], total: 1 };
			return Response.json({ data, meta: { requestId } });
		});

		await expect(
			load({
				fetch: fetcher,
				url: new URL('http://localhost:5173/dev/users?guildId=1525851181157580861')
			} as never)
		).resolves.toMatchObject({
			userSnapshot: { total: 1 },
			dummyGuildCatalog: [{ displayName: 'MonkeyBot Test' }],
			initialGuildId: '1525851181157580861',
			groups: [{ name: '20인 경매 풀' }],
			user: { serviceAdmin: true },
			guilds: [{ currentUserRole: 'OPERATOR' }]
		});
		expect(fetcher).toHaveBeenCalledTimes(5);
		expect(fetcher).toHaveBeenCalledWith('/api/dev/dummy-users?limit=100&offset=0', {
			headers: { accept: 'application/json' }
		});
	});
});
