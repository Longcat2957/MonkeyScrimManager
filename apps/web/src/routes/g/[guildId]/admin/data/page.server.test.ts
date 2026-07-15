import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const memberSnapshot = {
	revision: 1,
	updatedAt: '2026-07-15T06:00:00Z',
	updatedBy: null,
	members: []
};
const seasonSnapshot = {
	current: {
		id: '01900000-0000-7000-8000-000000000002',
		name: '2026 시즌 1',
		status: 'ACTIVE',
		startedAt: '2026-07-15T06:00:00Z',
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

describe('guild admin data server load', () => {
	it('loads the existing authoritative member and season snapshots for the data route', async () => {
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			const data = path.endsWith('/members') ? memberSnapshot : seasonSnapshot;
			return Response.json({ data, meta: { requestId } });
		});

		await expect(
			load({
				fetch: fetcher,
				params: { guildId },
				url: new URL(`http://localhost:5173/g/${guildId}/admin/data`)
			} as never)
		).resolves.toEqual({ guildId, memberSnapshot, seasonSnapshot });
		expect(fetcher).toHaveBeenCalledTimes(2);
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/members`, {
			headers: { accept: 'application/json' }
		});
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/seasons`, {
			headers: { accept: 'application/json' }
		});
	});
});
