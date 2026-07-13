import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const seasonSnapshot = {
	current: {
		id: '01900000-0000-7000-8000-000000000010',
		name: '시즌 1',
		status: 'ACTIVE',
		startedAt: '2026-07-13T01:00:00Z',
		endedAt: null,
		createdBy: null,
		participantCount: 0,
		generalSeriesCount: 0,
		auctionTournamentCount: 0,
		auctionTenCount: 0,
		auctionTwentyCount: 0,
		confirmedGameCount: 0,
		lastGameAt: null
	},
	completed: []
};

function run(fetcher: typeof globalThis.fetch) {
	return load({
		fetch: fetcher,
		params: { guildId },
		url: new URL(`http://localhost:5173/g/${guildId}/admin/seasons`)
	} as never);
}

describe('guild admin seasons server load', () => {
	it('loads the authoritative guild season snapshot', async () => {
		const fetcher = vi.fn(async () => Response.json({ data: seasonSnapshot, meta: { requestId } }));

		await expect(run(fetcher)).resolves.toEqual({ guildId, seasonSnapshot });
		expect(fetcher).toHaveBeenCalledOnce();
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/seasons`, {
			headers: { accept: 'application/json' }
		});
	});

	it('rejects a malformed season snapshot at the server boundary', async () => {
		const fetcher = vi.fn(async () =>
			Response.json({ data: { ...seasonSnapshot, extra: true }, meta: { requestId } })
		);

		await expect(run(fetcher)).rejects.toMatchObject({
			status: 502,
			body: { message: 'API 응답 형식이 올바르지 않습니다.' }
		});
	});

	it('redirects an expired session back through Discord OAuth', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: { code: 'unauthorized', message: '인증이 필요합니다.', requestId }
				},
				{ status: 401 }
			)
		);

		await expect(run(fetcher)).rejects.toMatchObject({
			status: 303,
			location: `/oauth/discord/start?next=%2Fg%2F${guildId}%2Fadmin%2Fseasons`
		});
	});
});
