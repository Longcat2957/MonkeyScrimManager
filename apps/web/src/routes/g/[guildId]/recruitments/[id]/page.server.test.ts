import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';

describe('recruitment stage page server load', () => {
	it('loads the authoritative recruitment snapshot before entry editing begins', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			void input;
			return Response.json({
				data: {
					recruitment: {
						id: recruitmentId,
						displayNumber: 214,
						title: '금요 일반 내전 모집',
						status: 'OPEN',
						targetCount: 10,
						bestOf: 3,
						hardFearless: false,
						revision: 3,
						createdById: '200000000000000004',
						channelId: '200000000000000002',
						messageId: '200000000000000003',
						updatedAt: '2026-07-14T12:00:00Z'
					},
					participants: [
						{ id: '200000000000000004', displayName: 'Longcat', preferredLanes: ['MID'] }
					],
					links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
				},
				meta: { requestId }
			});
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: recruitmentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/recruitments/${recruitmentId}`)
		} as never);

		expect(parent).not.toHaveBeenCalled();
		expect(fetcher).toHaveBeenCalledOnce();
		expect(String(fetcher.mock.calls[0]?.[0])).toBe(
			`/api/guilds/${guildId}/recruitments/${recruitmentId}`
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: {
				recruitment: { id: recruitmentId, status: 'OPEN' },
				participants: [{ displayName: 'Longcat' }]
			}
		});
	});
});
