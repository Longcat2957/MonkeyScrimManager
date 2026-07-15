import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';

function run(fetcher: typeof globalThis.fetch) {
	return load({ fetch: fetcher, url: new URL('http://localhost:5173/') } as never);
}

function userResponse(defaultGuildId: string | null): Response {
	return Response.json({
		data: {
			discordId: '390526792496054283',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId,
			serviceAdmin: false
		},
		meta: { requestId }
	});
}

describe('root server load', () => {
	it('sends an unauthenticated first visit to the landing page', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{ error: { code: 'unauthorized', message: '인증이 필요합니다.', requestId } },
				{ status: 401 }
			)
		);

		await expect(run(fetcher)).rejects.toMatchObject({ status: 303, location: '/landing' });
	});

	it('opens the default guild for an authenticated user', async () => {
		const fetcher = vi.fn(async () => userResponse(guildId));

		await expect(run(fetcher)).rejects.toMatchObject({
			status: 307,
			location: `/g/${guildId}`
		});
	});

	it('opens default guild settings when the authenticated user has no default guild', async () => {
		const fetcher = vi.fn(async () => userResponse(null));

		await expect(run(fetcher)).rejects.toMatchObject({
			status: 307,
			location: '/me?tab=guild'
		});
	});
});
