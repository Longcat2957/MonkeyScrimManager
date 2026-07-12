import { describe, expect, it, vi } from 'vitest';

const { runtimeEnv } = vi.hoisted(() => ({
	runtimeEnv: {
		OAUTH_BASE_URL: 'http://backend:8080/',
		API_BASE_URL: undefined
	}
}));

vi.mock('$env/dynamic/private', () => ({ env: runtimeEnv }));

import { fallback } from './+server';

describe('OAuth proxy', () => {
	it('returns redirect responses without following or rebuilding them', async () => {
		const upstreamResponse = new Response(null, {
			status: 302,
			headers: {
				location: 'https://discord.com/oauth2/authorize?client_id=123',
				'set-cookie':
					'monkeybot_oauth_state=opaque; Path=/oauth/callback; Max-Age=300; HttpOnly; SameSite=Lax'
			}
		});
		const proxyFetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			void input;
			void init;
			return upstreamResponse;
		});
		const request = new Request(
			'http://localhost:5173/oauth/discord/start?next=%2Fg%2F123456789012345678'
		);

		const response = await fallback({
			request,
			params: { path: 'discord/start' },
			url: new URL(request.url),
			fetch: proxyFetch
		} as never);

		expect(proxyFetch).toHaveBeenCalledOnce();
		const [upstream, init] = proxyFetch.mock.calls[0]!;
		expect(upstream.toString()).toBe(
			'http://backend:8080/oauth/discord/start?next=%2Fg%2F123456789012345678'
		);
		expect(init).toMatchObject({ method: 'GET', redirect: 'manual' });
		expect(response).toBe(upstreamResponse);
		expect(response.headers.get('location')).toBe(
			'https://discord.com/oauth2/authorize?client_id=123'
		);
		expect(response.headers.get('set-cookie')).toContain('monkeybot_oauth_state=opaque');
	});
});
