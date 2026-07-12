import { describe, expect, it, vi } from 'vitest';
import { apiGet, apiRoute } from './api';
import { meDataSchema } from '$lib/contracts/workflow';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const pageUrl = new URL(`http://localhost:5173/g/${guildId}/series?view=active`);

function errorResponse(status: number, message: string): Response {
	return Response.json({ error: { code: 'test_error', message, requestId } }, { status });
}

describe('server API client', () => {
	it('builds encoded same-origin API routes and unwraps a strict envelope', async () => {
		expect(apiRoute(['guilds', guildId, 'series'], { state: 'active', limit: 100 })).toBe(
			`/api/guilds/${guildId}/series?state=active&limit=100`
		);
		expect(apiRoute(['guilds', 'guild/name'])).toBe('/api/guilds/guild%2Fname');

		const data = {
			discordId: '100000000000000001',
			displayName: 'Longcat',
			avatarUrl: null,
			defaultGuildId: guildId,
			serviceAdmin: false
		};
		const fetcher = vi.fn(async () => Response.json({ data, meta: { requestId } }));

		await expect(apiGet(fetcher, '/api/me', meDataSchema, { url: pageUrl })).resolves.toEqual(data);
	});

	it('redirects an expired session to Discord OAuth with only the local return path', async () => {
		const fetcher = vi.fn(async () => errorResponse(401, '인증이 필요합니다.'));

		await expect(apiGet(fetcher, '/api/me', meDataSchema, { url: pageUrl })).rejects.toMatchObject({
			status: 303,
			location: `/oauth/discord/start?next=%2Fg%2F${guildId}%2Fseries%3Fview%3Dactive`
		});
	});

	it('preserves hidden 404 and dependency 503 without exposing malformed bodies', async () => {
		const notFound = vi.fn(async () => errorResponse(404, '요청한 길드를 찾을 수 없습니다.'));
		await expect(apiGet(notFound, '/api/me', meDataSchema, { url: pageUrl })).rejects.toMatchObject(
			{
				status: 404,
				body: { message: '요청한 길드를 찾을 수 없습니다.' }
			}
		);

		const unavailable = vi.fn(
			async () => new Response('postgres password=secret', { status: 503 })
		);
		await expect(
			apiGet(unavailable, '/api/me', meDataSchema, { url: pageUrl })
		).rejects.toMatchObject({
			status: 503,
			body: { message: 'API 서비스를 일시적으로 사용할 수 없습니다.' }
		});
	});

	it('normalizes network failures and malformed successful payloads', async () => {
		const networkFailure = vi.fn(async () => {
			throw new Error('connect ECONNREFUSED with secret');
		});
		await expect(
			apiGet(networkFailure, '/api/me', meDataSchema, { url: pageUrl })
		).rejects.toMatchObject({ status: 503, body: { message: 'API 서버에 연결할 수 없습니다.' } });

		const invalidPayload = vi.fn(async () => Response.json({ data: {}, meta: { requestId } }));
		await expect(
			apiGet(invalidPayload, '/api/me', meDataSchema, { url: pageUrl })
		).rejects.toMatchObject({
			status: 502,
			body: { message: 'API 응답 형식이 올바르지 않습니다.' }
		});
	});
});
