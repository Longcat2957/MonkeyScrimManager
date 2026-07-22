import { afterEach, describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';

describe('guild operations page server load', () => {
	afterEach(() => vi.useRealTimers());

	it('passes API-owned operation links through and only localizes the timestamp', async () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date('2026-07-12T12:00:00Z'));
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async () =>
			Response.json({
				data: {
					operations: [
						{
							id: 'series:01900000-0000-7000-8000-000000000201',
							kind: 'PICK_BAN',
							title: '금요 정기 내전',
							status: '픽밴/결과 입력',
							detail: '1세트 완료 · 2세트 입력 대기',
							updatedAt: '2026-07-12T11:52:00Z',
							href: `/g/${guildId}/series/01900000-0000-7000-8000-000000000201/pick-ban`,
							priority: 'urgent'
						}
					]
				},
				meta: { requestId }
			})
		);

		const result = await load({
			fetch: fetcher,
			params: { guildId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}`)
		} as never);
		if (!result) throw new Error('operations page load returned no data');

		expect(parent).not.toHaveBeenCalled();
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/operations`, {
			headers: { accept: 'application/json' }
		});
		expect(result.operations[0]).toMatchObject({
			updatedAt: '8분 전',
			href: `/g/${guildId}/series/01900000-0000-7000-8000-000000000201/pick-ban`
		});
	});
});
