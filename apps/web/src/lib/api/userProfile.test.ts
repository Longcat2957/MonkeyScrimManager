import { afterEach, describe, expect, it, vi } from 'vitest';
import { fetchUserProfile } from './userProfile';

const discordId = '1525847677542207548';
const requestId = '01900000-0000-7000-8000-000000000001';
const profile = {
	discordId,
	displayName: 'Longcat',
	activeGuildCount: 3,
	seasonCount: 7
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('user profile API client', () => {
	afterEach(() => vi.restoreAllMocks());

	it('loads the public profile through the canonical Discord ID route', async () => {
		const fetcher = vi.fn(async () => envelope(profile));

		await expect(fetchUserProfile(fetcher, discordId)).resolves.toEqual(profile);
		expect(fetcher).toHaveBeenCalledWith(`/api/users/${discordId}/profile`, {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	});

	it('preserves public errors and rejects a malformed profile', async () => {
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

		await expect(fetchUserProfile(fetcher, discordId)).rejects.toMatchObject({
			status: 404,
			code: 'not_found',
			requestId
		});

		fetcher.mockImplementationOnce(async () => envelope({ ...profile, unexpected: true }));
		await expect(fetchUserProfile(fetcher, discordId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('validates Discord IDs before requesting the API', async () => {
		const fetcher = vi.fn();

		await expect(fetchUserProfile(fetcher, 'user-tamsol')).rejects.toMatchObject({
			status: 0,
			code: 'invalid_discord_id'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes network errors without exposing transport details', async () => {
		const fetcher = vi.fn(async () => {
			throw new Error('connect ECONNREFUSED password=secret');
		});

		await expect(fetchUserProfile(fetcher, discordId)).rejects.toMatchObject({
			status: 0,
			code: 'network_error',
			message: '사용자 프로필 서버에 연결할 수 없습니다.'
		});
	});
});
