import { afterEach, describe, expect, it, vi } from 'vitest';

const { runtimeEnv } = vi.hoisted(() => ({
	runtimeEnv: {
		API_BASE_URL: 'http://backend:8080/api/' as string | undefined
	}
}));

vi.mock('$env/dynamic/private', () => ({ env: runtimeEnv }));

import { fallback } from './+server';

describe('API proxy', () => {
	afterEach(() => {
		runtimeEnv.API_BASE_URL = 'http://backend:8080/api/';
	});

	it('forwards the session cookie and query while removing hop-by-hop headers', async () => {
		const upstreamResponse = Response.json(
			{
				error: { code: 'not_found', message: '찾을 수 없습니다.', requestId: crypto.randomUUID() }
			},
			{ status: 404 }
		);
		const proxyFetch = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => {
			void _input;
			void _init;
			return upstreamResponse;
		});
		const request = new Request(
			'http://localhost:5173/api/guilds/123456789012345678/series?state=active&limit=100',
			{
				headers: {
					cookie: 'monkeybot_session=opaque-session',
					connection: 'keep-alive',
					'x-requested-with': 'sveltekit'
				}
			}
		);

		const response = await fallback({
			request,
			params: { path: 'guilds/123456789012345678/series' },
			url: new URL(request.url),
			fetch: proxyFetch
		} as never);

		expect(proxyFetch).toHaveBeenCalledOnce();
		const [upstream, init] = proxyFetch.mock.calls[0]!;
		if (!init) throw new Error('proxy fetch init was missing');
		expect(upstream.toString()).toBe(
			'http://backend:8080/api/guilds/123456789012345678/series?state=active&limit=100'
		);
		const headers = new Headers(init.headers);
		expect(headers.get('cookie')).toBe('monkeybot_session=opaque-session');
		expect(headers.get('connection')).toBeNull();
		expect(headers.get('x-requested-with')).toBe('sveltekit');
		expect(response).toBe(upstreamResponse);
		expect(response.status).toBe(404);
	});

	it('returns 503 without contacting an upstream when API_BASE_URL is missing', async () => {
		runtimeEnv.API_BASE_URL = undefined;
		const proxyFetch = vi.fn();
		const request = new Request('http://localhost:5173/api/me');

		const response = await fallback({
			request,
			params: { path: 'me' },
			url: new URL(request.url),
			fetch: proxyFetch
		} as never);

		expect(response.status).toBe(503);
		expect(proxyFetch).not.toHaveBeenCalled();
	});

	it('preserves both logout cookie-clearing headers from the API response', async () => {
		const upstreamResponse = Response.json(
			{ data: { loggedOut: true }, meta: { requestId: crypto.randomUUID() } },
			{ status: 200 }
		);
		upstreamResponse.headers.append(
			'set-cookie',
			'monkeybot_session=; Path=/; HttpOnly; Max-Age=0; SameSite=Lax'
		);
		upstreamResponse.headers.append(
			'set-cookie',
			'monkeybot_csrf=; Path=/; Max-Age=0; SameSite=Lax'
		);
		const proxyFetch = vi.fn(async () => upstreamResponse);
		const request = new Request('http://localhost:5173/api/logout', {
			method: 'POST',
			headers: {
				cookie: 'monkeybot_session=opaque-session; monkeybot_csrf=csrf-token',
				origin: 'http://localhost:5173',
				'X-CSRF-Token': 'csrf-token'
			}
		});

		const response = await fallback({
			request,
			params: { path: 'logout' },
			url: new URL(request.url),
			fetch: proxyFetch
		} as never);

		expect(response).toBe(upstreamResponse);
		const cookies = response.headers.get('set-cookie');
		expect(cookies).toContain('monkeybot_session=;');
		expect(cookies).toContain('monkeybot_csrf=;');
	});

	it('forwards localhost Riot mutations to the configured API port with body and CSRF headers', async () => {
		runtimeEnv.API_BASE_URL = 'http://localhost:30020/api/';
		let forwardedUrl = '';
		let forwardedBody = '';
		let forwardedHeaders = new Headers();
		const proxyFetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			forwardedUrl = input.toString();
			forwardedHeaders = new Headers(init?.headers);
			forwardedBody = await new Response(init?.body).text();
			return Response.json({
				data: { riotAccounts: [] },
				meta: { requestId: crypto.randomUUID() }
			});
		});
		const body = JSON.stringify({ gameName: 'Longcat', tagLine: 'KR1', platform: 'kr' });
		const request = new Request('http://localhost:5173/api/me/riot-accounts', {
			method: 'POST',
			headers: {
				'content-type': 'application/json',
				cookie: 'monkeybot_session=opaque-session; monkeybot_csrf=csrf-token',
				'X-CSRF-Token': 'csrf-token'
			},
			body
		});

		const response = await fallback({
			request,
			params: { path: 'me/riot-accounts' },
			url: new URL(request.url),
			fetch: proxyFetch
		} as never);

		expect(response.status).toBe(200);
		expect(forwardedUrl).toBe('http://localhost:30020/api/me/riot-accounts');
		expect(forwardedHeaders.get('content-type')).toBe('application/json');
		expect(forwardedHeaders.get('X-CSRF-Token')).toBe('csrf-token');
		expect(forwardedHeaders.get('cookie')).toContain('monkeybot_session=opaque-session');
		expect(forwardedBody).toBe(body);
	});
});
