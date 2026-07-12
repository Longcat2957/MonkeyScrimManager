import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('my page server load', () => {
	it('does not call Riot accounts endpoint on non-Riot tab', async () => {
		const user = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: '200000000000000001',
			serviceAdmin: false
		};
		const guilds = {
			guilds: [
				{
					id: '200000000000000001',
					displayName: 'Monkey Scrim Team',
					iconUrl: null,
					memberCount: 12,
					currentUserRole: 'MEMBER'
				}
			]
		};

		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			switch (path) {
				case '/api/me':
					return envelope(user);
				case '/api/me/guilds':
					return envelope(guilds);
				default:
					throw new Error(`unexpected request: ${path}`);
			}
		});

		const result = await load({
			fetch: fetcher,
			url: new URL('http://localhost:5173/me?tab=guild')
		} as never);
		if (!result) throw new Error('load should return data');

		expect(fetcher).toHaveBeenCalledTimes(2);
		expect(result).toEqual(
			expect.objectContaining({
				defaultGuildId: user.defaultGuildId,
				guilds: guilds.guilds
			})
		);
		const called = fetcher.mock.calls.map(([path]) => String(path));
		expect(called).toEqual(['/api/me', '/api/me/guilds']);
		expect(result.riotAccounts).toEqual([]);
	});

	it('calls Riot accounts endpoint only for Riot tab', async () => {
		const user = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: null,
			serviceAdmin: false
		};
		const guilds = { guilds: [] };
		const riotAccounts = { riotAccounts: [] };

		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			switch (path) {
				case '/api/me':
					return envelope(user);
				case '/api/me/guilds':
					return envelope(guilds);
				case '/api/me/riot-accounts':
					return envelope(riotAccounts);
				default:
					throw new Error(`unexpected request: ${path}`);
			}
		});

		const result = await load({
			fetch: fetcher,
			url: new URL('http://localhost:5173/me?tab=riot')
		} as never);
		if (!result) throw new Error('load should return data');

		expect(fetcher).toHaveBeenCalledTimes(3);
		expect(fetcher).toHaveBeenNthCalledWith(1, '/api/me', { headers: { accept: 'application/json' } });
		expect(fetcher).toHaveBeenNthCalledWith(2, '/api/me/guilds', { headers: { accept: 'application/json' } });
		expect(fetcher).toHaveBeenNthCalledWith(3, '/api/me/riot-accounts', { headers: { accept: 'application/json' } });
		expect(result.riotAccounts).toEqual([]);
	});
});
