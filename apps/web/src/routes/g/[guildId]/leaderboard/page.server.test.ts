import { describe, expect, it, vi } from 'vitest';
import type { GuildLeaderboardSnapshot } from '$lib/contracts/workflow';
import { load } from './+page.server';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const snapshot: GuildLeaderboardSnapshot = {
	season: { id: '01900000-0000-7000-8000-000000000010', name: '시즌 1' },
	players: []
};

function run(fetcher: typeof globalThis.fetch) {
	return load({
		fetch: fetcher,
		params: { guildId },
		parent: vi.fn().mockResolvedValue({ guildId }),
		url: new URL(`http://localhost:5173/g/${guildId}/leaderboard`)
	} as never);
}

describe('guild leaderboard server load', () => {
	it('SSR-loads the authoritative leaderboard snapshot', async () => {
		const fetcher = vi.fn(async () => Response.json({ data: snapshot, meta: { requestId } }));

		await expect(run(fetcher)).resolves.toEqual({ snapshot, loadError: null });
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/leaderboard`, {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	});

	it('keeps a route-level backend failure in the page state', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'guild_member_not_found',
						message: '길드 리더보드를 찾을 수 없습니다.',
						requestId
					}
				},
				{ status: 404 }
			)
		);

		await expect(run(fetcher)).resolves.toEqual({
			snapshot: null,
			loadError: '길드 리더보드를 찾을 수 없습니다.'
		});
	});

	it('redirects an expired session to Discord OAuth', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'unauthorized',
						message: '로그인이 필요합니다.',
						requestId
					}
				},
				{ status: 401 }
			)
		);

		await expect(run(fetcher)).rejects.toMatchObject({
			status: 303,
			location: expect.stringContaining('/oauth/discord/start?next=')
		});
	});
});
