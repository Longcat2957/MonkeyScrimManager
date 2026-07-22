import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const emptyChampionPreferences = {
	TOP: [],
	JUNGLE: [],
	MID: [],
	BOTTOM: [],
	SUPPORT: []
};
const defaultPrivacySettings = {
	publicProfile: true,
	showRiotIds: true,
	allowExternalRecordSearch: false
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('my page server load', () => {
	it('does not call Riot, privacy, or champion preference endpoints on the guild tab', async () => {
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
		expect(result.championPreferences).toEqual(emptyChampionPreferences);
		expect(result.privacySettings).toEqual(defaultPrivacySettings);
	});

	it('loads the authoritative privacy snapshot only for the privacy tab', async () => {
		const user = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: '200000000000000001',
			serviceAdmin: false
		};
		const guilds = { guilds: [] };
		const privacySettings = {
			publicProfile: false,
			showRiotIds: false,
			allowExternalRecordSearch: true
		};
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === '/api/me') return envelope(user);
			if (path === '/api/me/guilds') return envelope(guilds);
			if (path === '/api/me/privacy') return envelope({ settings: privacySettings });
			throw new Error(`unexpected request: ${path}`);
		});

		const result = await load({
			fetch: fetcher,
			url: new URL('http://localhost:5173/me?tab=privacy')
		} as never);
		if (!result) throw new Error('load should return data');

		expect(fetcher).toHaveBeenCalledTimes(3);
		expect(fetcher).toHaveBeenNthCalledWith(3, '/api/me/privacy', {
			headers: { accept: 'application/json' }
		});
		expect(result.privacySettings).toEqual(privacySettings);
		expect(result.riotAccounts).toEqual([]);
		expect(result.championPreferences).toEqual(emptyChampionPreferences);
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
		expect(fetcher).toHaveBeenNthCalledWith(1, '/api/me', {
			headers: { accept: 'application/json' }
		});
		expect(fetcher).toHaveBeenNthCalledWith(2, '/api/me/guilds', {
			headers: { accept: 'application/json' }
		});
		expect(fetcher).toHaveBeenNthCalledWith(3, '/api/me/riot-accounts', {
			headers: { accept: 'application/json' }
		});
		expect(result.riotAccounts).toEqual([]);
		expect(result.championPreferences).toEqual(emptyChampionPreferences);
	});

	it('loads champion preferences only for the champions tab', async () => {
		const user = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: null,
			serviceAdmin: false
		};
		const preferences = {
			...emptyChampionPreferences,
			MID: ['Ahri', 'Orianna']
		};
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === '/api/me') return envelope(user);
			if (path === '/api/me/guilds') return envelope({ guilds: [] });
			if (path === '/api/me/champion-preferences') return envelope({ preferences });
			throw new Error(`unexpected request: ${path}`);
		});

		const result = await load({
			fetch: fetcher,
			url: new URL('http://localhost:5173/me?tab=champions')
		} as never);
		if (!result) throw new Error('load should return data');

		expect(fetcher).toHaveBeenCalledTimes(3);
		expect(fetcher).toHaveBeenNthCalledWith(3, '/api/me/champion-preferences', {
			headers: { accept: 'application/json' }
		});
		expect(result.championPreferences).toEqual(preferences);
		expect(result.riotAccounts).toEqual([]);
	});

	it('uses the champions tab contract for the default and unknown tab values', async () => {
		const user = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: null,
			serviceAdmin: false
		};
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === '/api/me') return envelope(user);
			if (path === '/api/me/guilds') return envelope({ guilds: [] });
			if (path === '/api/me/champion-preferences') {
				return envelope({ preferences: emptyChampionPreferences });
			}
			throw new Error(`unexpected request: ${path}`);
		});

		await load({
			fetch: fetcher,
			url: new URL('http://localhost:5173/me?tab=unknown')
		} as never);

		expect(fetcher).toHaveBeenCalledTimes(3);
		expect(fetcher).not.toHaveBeenCalledWith('/api/me/riot-accounts', expect.anything());
	});

	it('surfaces a Riot dependency failure instead of replacing it with an empty list', async () => {
		const user = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: null,
			serviceAdmin: false
		};
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === '/api/me') return envelope(user);
			if (path === '/api/me/guilds') return envelope({ guilds: [] });
			if (path === '/api/me/riot-accounts') {
				return Response.json(
					{
						error: {
							code: 'riot_unavailable',
							message: 'Riot 서비스를 일시적으로 사용할 수 없습니다.',
							requestId
						}
					},
					{ status: 503 }
				);
			}
			throw new Error(`unexpected request: ${path}`);
		});

		await expect(
			load({
				fetch: fetcher,
				url: new URL('http://localhost:5173/me?tab=riot')
			} as never)
		).rejects.toMatchObject({
			status: 503,
			body: {
				message: `Riot 서비스를 일시적으로 사용할 수 없습니다. (요청 ID: ${requestId})`
			}
		});
		expect(fetcher).toHaveBeenCalledTimes(3);
	});
});
