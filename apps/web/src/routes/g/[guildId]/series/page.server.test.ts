import { afterEach, describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const activeSeriesId = '01900000-0000-7000-8000-000000000201';
const completedSeriesId = '01900000-0000-7000-8000-000000000202';

function envelope(data: unknown) {
	return Response.json({ data, meta: { requestId } });
}

describe('general series page server load', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('loads actionable, active, and completed work and builds a draw-safe view', async () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date('2026-07-12T12:00:00Z'));

		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const target = String(input);

			if (target.includes('/recruitments?')) {
				return envelope({
					recruitments: [
						{
							id: recruitmentId,
							displayNumber: 214,
							title: '금요 일반 내전 모집',
							status: 'CLOSED',
							targetCount: 10,
							bestOf: 3,
							hardFearless: false,
							joinedCount: 10,
							assignedCount: 4,
							revision: 3,
							updatedAt: '2026-07-12T10:00:00Z',
							href: `/g/${guildId}/recruitments/${recruitmentId}/entry`
						}
					],
					nextCursor: null
				});
			}

			if (target.includes('state=active')) {
				return envelope({
					series: [
						{
							id: activeSeriesId,
							displayNumber: 106,
							title: '금요 정기 내전',
							bestOf: 3,
							hardFearless: false,
							status: 'IN_PROGRESS',
							team1Score: 0,
							team2Score: 1,
							winningTeam: null,
							terminalReason: null,
							currentSetNumber: 2,
							outcome: null,
							updatedAt: '2026-07-12T11:00:00Z',
							endedAt: null,
							href: `/g/${guildId}/series/${activeSeriesId}/pick-ban`
						}
					],
					nextCursor: null
				});
			}

			if (target.includes('state=completed')) {
				return envelope({
					series: [
						{
							id: completedSeriesId,
							displayNumber: 105,
							title: '주말 자유 내전',
							bestOf: 3,
							hardFearless: false,
							status: 'COMPLETED',
							team1Score: 1,
							team2Score: 1,
							winningTeam: null,
							terminalReason: 'EARLY_DRAW',
							currentSetNumber: null,
							outcome: 'DRAW',
							updatedAt: '2026-07-11T12:00:00Z',
							endedAt: '2026-07-11T12:00:00Z',
							href: `/g/${guildId}/series/${completedSeriesId}/result`
						}
					],
					nextCursor: null
				});
			}

			throw new Error(`unexpected request: ${target}`);
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/series?create=1`)
		} as never);
		if (!result) throw new Error('series page load returned no data');

		expect(parent).not.toHaveBeenCalled();
		expect(fetcher).toHaveBeenCalledTimes(3);
		expect(fetcher.mock.calls.map(([input]) => String(input))).toEqual([
			`/api/guilds/${guildId}/recruitments?state=actionable&limit=100`,
			`/api/guilds/${guildId}/series?state=active&limit=100`,
			`/api/guilds/${guildId}/series?state=completed&limit=100`
		]);

		expect(result.guildId).toBe(guildId);
		expect(result.openCreateDialog).toBe(true);
		expect(result.view.activeWork.map((item: { kind: string }) => item.kind)).toEqual([
			'SERIES',
			'RECRUITMENT'
		]);
		expect(result.view.activeWork[0]).toMatchObject({
			displayNumber: 106,
			format: 'BO3 · 일반',
			actionLabel: '픽밴 진행',
			href: `/g/${guildId}/series/${activeSeriesId}/pick-ban`
		});
		expect(result.view.activeWork[1]).toMatchObject({
			displayNumber: 214,
			actionLabel: '엔트리 편성',
			href: `/g/${guildId}/recruitments/${recruitmentId}/entry`
		});
		expect(result.view.completedSeries[0]).toMatchObject({
			displayNumber: 105,
			outcome: 'DRAW',
			winner: '무승부',
			href: `/g/${guildId}/series/${completedSeriesId}/result`
		});
	});
});
