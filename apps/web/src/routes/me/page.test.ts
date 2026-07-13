import { describe, expect, it, vi } from 'vitest';
import { load } from './+page';

describe('my page universal load', () => {
	it.each(['riot', 'guild', 'privacy'] as const)(
		'does not load Data Dragon resources for the %s tab',
		async (tab) => {
			const fetcher = vi.fn();
			const data = {
				user: {
					discordId: '100000000000000001',
					displayName: 'Longcat',
					avatarUrl: null,
					defaultGuildId: null,
					serviceAdmin: false
				},
				guilds: [],
				defaultGuildId: null,
				riotAccounts: [],
				championPreferences: {
					TOP: [],
					JUNGLE: [],
					MID: [],
					BOTTOM: [],
					SUPPORT: []
				},
				privacySettings: {
					publicProfile: true,
					showRiotIds: true,
					allowExternalRecordSearch: false
				}
			};

			const result = await load({
				fetch: fetcher,
				url: new URL(`http://localhost:5173/me?tab=${tab}`),
				data
			} as never);
			if (!result) throw new Error('load should return data');

			expect(fetcher).not.toHaveBeenCalled();
			expect(result).toEqual(expect.objectContaining({ initialTab: tab, championCatalog: null }));
			expect(result.privacySettings).toEqual(data.privacySettings);
		}
	);
});
