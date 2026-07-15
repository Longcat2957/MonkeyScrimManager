import { afterEach, describe, expect, it, vi } from 'vitest';

import { fetchServiceStatus, serviceStatusErrorText } from './serviceStatus';

const requestId = '01900000-0000-7000-8000-000000000001';
const snapshot = {
	checkedAt: '2026-07-15T12:00:00Z',
	services: [
		{
			key: 'API' as const,
			status: 'normal' as const,
			latencyMs: null,
			value: '요청 수신 가능',
			lastHealthyAt: '2026-07-15T12:00:00Z',
			issue: null
		},
		{
			key: 'POSTGRESQL' as const,
			status: 'normal' as const,
			latencyMs: 12,
			value: '연결 정상',
			lastHealthyAt: '2026-07-15T12:00:00Z',
			issue: null
		},
		{
			key: 'BOT_HEARTBEAT' as const,
			status: 'delayed' as const,
			latencyMs: null,
			value: '1분 42초 전',
			lastHealthyAt: null,
			issue: '마지막 heartbeat가 정상 기준 90초를 초과했습니다.'
		},
		{
			key: 'VALKEY' as const,
			status: 'normal' as const,
			latencyMs: 4,
			value: '연결 정상',
			lastHealthyAt: '2026-07-15T12:00:00Z',
			issue: null
		}
	]
};

describe('service status API client', () => {
	afterEach(() => {
		vi.restoreAllMocks();
	});

	it('loads the authenticated service-admin status snapshot', async () => {
		const fetcher = vi.fn(async () => Response.json({ data: snapshot, meta: { requestId } }));

		await expect(fetchServiceStatus(fetcher as typeof globalThis.fetch)).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledWith('/api/admin/status', {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	});

	it('preserves safe backend failures with the correlation ID', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'service_admin_required',
						message: '서비스 관리자 권한이 필요합니다.',
						requestId
					}
				},
				{ status: 403 }
			)
		);

		await expect(fetchServiceStatus(fetcher as typeof globalThis.fetch)).rejects.toMatchObject({
			status: 403,
			code: 'service_admin_required',
			message: '서비스 관리자 권한이 필요합니다.'
		});

		try {
			await fetchServiceStatus(fetcher as typeof globalThis.fetch);
		} catch (cause) {
			expect(serviceStatusErrorText(cause)).toBe(
				`서비스 관리자 권한이 필요합니다. (요청 ID: ${requestId})`
			);
		}
	});

	it('rejects malformed success payloads and transport failures safely', async () => {
		const malformed = vi.fn(async () =>
			Response.json({ data: { ...snapshot, services: [] }, meta: { requestId } })
		);
		await expect(fetchServiceStatus(malformed as typeof globalThis.fetch)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response',
			message: '운영 상태 응답 형식이 올바르지 않습니다.'
		});

		const offline = vi.fn(async () => {
			throw new Error('connection refused password=secret');
		});
		await expect(fetchServiceStatus(offline as typeof globalThis.fetch)).rejects.toMatchObject({
			status: 0,
			code: 'network_error',
			message: '운영 상태 API 서버에 연결할 수 없습니다.'
		});
	});
});
