import { afterEach, describe, expect, it, vi } from 'vitest';

import {
	fetchGuildRecordResetPreview,
	resetGuildRecords,
	serviceAdminResetErrorText
} from './serviceAdminReset';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const counts = {
	totalRows: 11222,
	seasons: 1,
	recruitments: 120,
	series: 68,
	games: 109,
	auctionRecruitments: 5,
	auctionTournaments: 6,
	mmrRows: 112,
	importJobs: 1
};
const preview = {
	guildId,
	displayName: 'MonkeyBot Test',
	memberCount: 56,
	activeSeasonName: '시즌 1',
	confirmation: `RESET ${guildId}`,
	counts
};
const result = {
	guildId,
	displayName: 'MonkeyBot Test',
	resetAt: '2026-07-17T12:00:00Z',
	deleted: counts,
	freshSeason: { id: '01900000-0000-7000-8000-000000000003', name: '시즌 1' }
};

function response(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('service-admin guild record reset API', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('previews the exact target and sends the guarded reset command', async () => {
		const fetcher = vi
			.fn()
			.mockResolvedValueOnce(response(preview))
			.mockResolvedValueOnce(response(result));
		vi.stubGlobal('document', { cookie: 'theme=dark; monkeybot_csrf=csrf-token' });

		await expect(fetchGuildRecordResetPreview(guildId, fetcher)).resolves.toEqual(preview);
		await expect(
			resetGuildRecords(guildId, preview.confirmation, idempotencyKey, fetcher)
		).resolves.toEqual(result);

		expect(fetcher.mock.calls[0]?.[0]).toBe(`/api/admin/guilds/${guildId}/record-reset`);
		const resetInit = fetcher.mock.calls[1]?.[1];
		const headers = new Headers(resetInit?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		expect(JSON.parse(String(resetInit?.body))).toEqual({ confirmation: preview.confirmation });
	});

	it('preserves the backend reason and request ID', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'reset_confirmation_mismatch',
						message: '확인 문구가 대상 서버와 일치하지 않습니다.',
						requestId
					}
				},
				{ status: 422 }
			)
		);
		vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token' });

		try {
			await resetGuildRecords(guildId, 'wrong', idempotencyKey, fetcher);
			expect.unreachable();
		} catch (cause) {
			expect(serviceAdminResetErrorText(cause)).toBe(
				`확인 문구가 대상 서버와 일치하지 않습니다. (요청 ID: ${requestId})`
			);
		}
	});

	it('rejects missing CSRF, invalid IDs, and malformed success responses', async () => {
		const fetcher = vi.fn(async () => response({ ...preview, counts: { ...counts, games: -1 } }));
		vi.stubGlobal('document', { cookie: '' });
		await expect(
			resetGuildRecords(guildId, preview.confirmation, idempotencyKey, fetcher)
		).rejects.toMatchObject({
			code: 'csrf_token_missing'
		});
		await expect(fetchGuildRecordResetPreview('not-a-guild', fetcher)).rejects.toMatchObject({
			code: 'invalid_guild_id'
		});
		await expect(fetchGuildRecordResetPreview(guildId, fetcher)).rejects.toMatchObject({
			code: 'invalid_response',
			requestId
		});
	});
});
