import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GuildAuditLogsSnapshot } from '$lib/contracts/workflow';
import { fetchGuildAuditLogs, guildAuditLogsErrorText } from './guildAuditLogs';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const snapshot: GuildAuditLogsSnapshot = {
	entries: [
		{
			id: '01900000-0000-7000-8000-000000000010',
			principalType: 'USER',
			principalId: '1525847677542207548',
			principalDisplayName: 'Longcat',
			action: 'MEMBER_MMR_ADJUSTED',
			targetType: 'GUILD_MEMBER_MMR',
			targetId: '1525900000000000001',
			targetDisplayName: '마루',
			checkpointId: null,
			requestId,
			beforeSummary: { lane: 'TOP', mmr: 1578 },
			afterSummary: { lane: 'TOP', mmr: 1596 },
			result: 'APPLIED',
			note: '누락 경기 반영',
			createdAt: '2026-07-13T05:00:00Z'
		}
	],
	nextCursor: '2026-07-13T05:00:00Z|01900000-0000-7000-8000-000000000010'
};

function installFetch(response: () => Response) {
	const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
		void args;
		return response();
	});
	vi.stubGlobal('fetch', fetcher);
	return fetcher;
}

describe('guild audit logs API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads a strict page and encodes the cursor', async () => {
		const page = {
			...snapshot,
			entries: snapshot.entries.map((entry) => ({ ...entry, note: '' }))
		};
		const fetcher = installFetch(() => Response.json({ data: page, meta: { requestId } }));

		await expect(
			fetchGuildAuditLogs(guildId, { limit: 25, cursor: 'cursor/value + next' })
		).resolves.toEqual(page);
		expect(fetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/audit-logs?limit=25&cursor=cursor%2Fvalue+%2B+next`,
			{
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it('preserves a backend error and rejects malformed success data', async () => {
		const forbidden = installFetch(() =>
			Response.json(
				{ error: { code: 'guild_operator_required', message: '권한이 없습니다.', requestId } },
				{ status: 403 }
			)
		);
		const forbiddenError = await fetchGuildAuditLogs(guildId).catch((cause) => cause);
		expect(forbiddenError).toMatchObject({
			status: 403,
			code: 'guild_operator_required',
			message: '권한이 없습니다.',
			requestId
		});
		expect(guildAuditLogsErrorText(forbiddenError)).toBe(
			`권한이 없습니다. (요청 ID: ${requestId})`
		);

		forbidden.mockImplementationOnce(async () =>
			Response.json({ data: { ...snapshot, unexpected: true }, meta: { requestId } })
		);
		await expect(fetchGuildAuditLogs(guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('validates page input before calling fetch', async () => {
		const fetcher = installFetch(() => Response.json({ data: snapshot, meta: { requestId } }));

		await expect(fetchGuildAuditLogs(guildId, { limit: 101 })).rejects.toMatchObject({
			status: 0,
			code: 'invalid_limit'
		});
		await expect(fetchGuildAuditLogs(guildId, { cursor: '' })).rejects.toMatchObject({
			status: 0,
			code: 'invalid_cursor'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes transport failures', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED password=secret');
		});
		vi.stubGlobal('fetch', fetcher);

		await expect(fetchGuildAuditLogs(guildId)).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
