import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const discordId = '1525847677542207548';
const requestId = '01900000-0000-7000-8000-000000000001';
const profile = {
	discordId,
	displayName: 'Longcat',
	activeGuildCount: 3,
	seasonCount: 7
};

describe('public user profile page server load', () => {
	it('loads the profile by Discord ID', async () => {
		const fetcher = vi.fn(async () => Response.json({ data: profile, meta: { requestId } }));

		const result = await load({ fetch: fetcher, params: { discordId } } as never);
		if (!result) throw new Error('user profile load returned no data');

		expect(fetcher).toHaveBeenCalledWith(`/api/users/${discordId}/profile`, {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
		expect(result.profile).toEqual(profile);
	});

	it('keeps a private or absent profile hidden behind the API message', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'not_found',
						message: '요청한 사용자 프로필을 찾을 수 없습니다.',
						requestId
					}
				},
				{ status: 404 }
			)
		);

		await expect(load({ fetch: fetcher, params: { discordId } } as never)).rejects.toMatchObject({
			status: 404,
			body: { message: `요청한 사용자 프로필을 찾을 수 없습니다. (요청 ID: ${requestId})` }
		});
	});
});
