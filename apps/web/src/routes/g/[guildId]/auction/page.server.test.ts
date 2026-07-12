import { afterEach, describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const activeTournamentId = '01900000-0000-7000-8000-000000000601';
const completedTournamentId = '01900000-0000-7000-8000-000000000602';
const championTeamId = '01900000-0000-7000-8000-000000000701';

function envelope(data: unknown) {
	return Response.json({ data, meta: { requestId } });
}

describe('auction page server load', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('loads actionable, active, and completed auction work with API-owned links', async () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date('2026-07-12T12:00:00Z'));

		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const target = String(input);

			if (target.includes('/auction-recruitments?')) {
				return envelope({
					recruitments: [
						{
							id: recruitmentId,
							displayNumber: 42,
							title: '수요 10인 경매',
							format: 'TEN_PERSON',
							participantCount: 10,
							joinedParticipantCount: 10,
							teamCount: 2,
							status: 'CLOSED',
							revision: 3,
							updatedAt: '2026-07-12T10:00:00Z',
							href: `/g/${guildId}/auction/recruitments/${recruitmentId}/draft`
						}
					],
					nextCursor: null
				});
			}

			if (target.includes('state=active')) {
				return envelope({
					tournaments: [
						{
							id: activeTournamentId,
							displayNumber: 118,
							title: '토요 20인 경매',
							format: 'TWENTY_PERSON',
							participantCount: 20,
							teamCount: 4,
							status: 'BIDDING',
							revision: 7,
							activeMemberCount: 8,
							bidMemberCount: 4,
							incompleteMatchCount: 0,
							nextMatch: null,
							championTeam: null,
							updatedAt: '2026-07-12T11:00:00Z',
							endedAt: null,
							href: `/g/${guildId}/auction/tournaments/${activeTournamentId}/draft`
						}
					],
					nextCursor: null
				});
			}

			if (target.includes('state=completed')) {
				return envelope({
					tournaments: [
						{
							id: completedTournamentId,
							displayNumber: 117,
							title: '금요 10인 경매',
							format: 'TEN_PERSON',
							participantCount: 10,
							teamCount: 2,
							status: 'COMPLETED',
							revision: 12,
							activeMemberCount: 10,
							bidMemberCount: 8,
							incompleteMatchCount: 0,
							nextMatch: null,
							championTeam: { id: championTeamId, name: '1팀' },
							updatedAt: '2026-07-11T18:00:00Z',
							endedAt: '2026-07-11T18:00:00Z',
							href: `/g/${guildId}/auction/tournaments/${completedTournamentId}/result`
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
			url: new URL(`http://localhost:5173/g/${guildId}/auction`)
		} as never);
		if (!result) throw new Error('auction page load returned no data');

		expect(parent).toHaveBeenCalledOnce();
		expect(fetcher.mock.calls.map(([input]) => String(input))).toEqual([
			`/api/guilds/${guildId}/auction-recruitments?state=actionable&limit=100`,
			`/api/guilds/${guildId}/auction-tournaments?state=active&limit=100`,
			`/api/guilds/${guildId}/auction-tournaments?state=completed&limit=100`
		]);
		expect(result.guildId).toBe(guildId);
		expect(result.view.activeWork[0]?.iconKind).toBe('bidding');
		expect(result.view.activeWork[1]?.iconKind).toBe('recruitment');
		expect(result.view.activeWork[0]).toMatchObject({
			displayNumber: 118,
			href: `/g/${guildId}/auction/tournaments/${activeTournamentId}/draft`
		});
		expect(result.view.completedWork[0]).toMatchObject({
			displayNumber: 117,
			kind: '완료',
			href: `/g/${guildId}/auction/tournaments/${completedTournamentId}/result`
		});
	});
});
