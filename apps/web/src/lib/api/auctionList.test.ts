import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AuctionRecruitmentListPage, AuctionTournamentListPage } from './auctionList';
import { fetchActionableAuctionRecruitments, fetchAuctionTournaments } from './auctionList';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const tournamentId = '01900000-0000-7000-8000-000000000601';

const recruitmentPage: AuctionRecruitmentListPage = {
	recruitments: [
		{
			id: recruitmentId,
			displayNumber: 42,
			title: '수요 10인 경매',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			participantCount: 10,
			joinedParticipantCount: 4,
			teamCount: 2,
			status: 'OPEN',
			revision: 3,
			updatedAt: '2026-07-12T10:00:00Z',
			href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
		}
	],
	nextCursor: '2026-07-12T10:00:00Z|cursor'
};

const tournamentPage: AuctionTournamentListPage = {
	tournaments: [
		{
			id: tournamentId,
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
			href: `/g/${guildId}/auction/tournaments/${tournamentId}/draft`
		}
	],
	nextCursor: null
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('auction list API client', () => {
	afterEach(() => vi.restoreAllMocks());

	it('loads actionable recruitments and completed tournaments with independent cursors', async () => {
		const recruitmentFetcher = vi.fn(async () => envelope(recruitmentPage));
		const tournamentFetcher = vi.fn(async () => envelope(tournamentPage));

		await expect(
			fetchActionableAuctionRecruitments(recruitmentFetcher, guildId, {
				limit: 25,
				cursor: 'cursor/value + next'
			})
		).resolves.toEqual(recruitmentPage);
		await expect(
			fetchAuctionTournaments(tournamentFetcher, guildId, 'completed', {
				limit: 40,
				cursor: 'completed cursor'
			})
		).resolves.toEqual(tournamentPage);

		expect(recruitmentFetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/auction-recruitments?state=actionable&limit=25&cursor=cursor%2Fvalue+%2B+next`,
			{
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
		expect(tournamentFetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/auction-tournaments?state=completed&limit=40&cursor=completed+cursor`,
			{
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it('preserves backend errors and rejects contract drift', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'guild_access_denied',
						message: '길드 접근 권한이 없습니다.',
						requestId
					}
				},
				{ status: 403 }
			)
		);

		await expect(fetchAuctionTournaments(fetcher, guildId, 'active')).rejects.toMatchObject({
			status: 403,
			code: 'guild_access_denied',
			message: '길드 접근 권한이 없습니다.',
			requestId
		});

		fetcher.mockImplementationOnce(async () =>
			envelope({ ...recruitmentPage, unexpected: 'contract drift' })
		);
		await expect(fetchActionableAuctionRecruitments(fetcher, guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('validates list paging before network I/O', async () => {
		const fetcher = vi.fn();

		await expect(
			fetchActionableAuctionRecruitments(fetcher, guildId, { limit: 101 })
		).rejects.toMatchObject({ status: 0, code: 'invalid_limit' });
		await expect(
			fetchAuctionTournaments(fetcher, guildId, 'active', { cursor: '' })
		).rejects.toMatchObject({ status: 0, code: 'invalid_cursor' });
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes network failures without exposing transport details', async () => {
		const fetcher = vi.fn(async () => {
			throw new Error('connect ECONNREFUSED password=secret');
		});

		await expect(fetchAuctionTournaments(fetcher, guildId, 'active')).rejects.toMatchObject({
			status: 0,
			code: 'network_error',
			message: 'API 서버에 연결할 수 없습니다.'
		});
	});
});
