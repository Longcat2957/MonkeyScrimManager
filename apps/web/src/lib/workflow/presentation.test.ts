import { describe, expect, it } from 'vitest';
import { buildAuctionView, formatRelativeTime } from './presentation';
import type { AuctionTournamentDto } from '$lib/contracts/workflow';

const now = Date.parse('2026-07-12T12:00:00Z');

describe('workflow presentation', () => {
	it('formats backend UTC timestamps with one stable Korean clock', () => {
		expect(formatRelativeTime('2026-07-12T11:59:40Z', now)).toBe('방금');
		expect(formatRelativeTime('2026-07-12T11:30:00Z', now)).toBe('30분 전');
		expect(formatRelativeTime('2026-07-12T09:00:00Z', now)).toBe('3시간 전');
		expect(formatRelativeTime('2026-07-10T12:00:00Z', now)).toBe('2일 전');
		expect(formatRelativeTime('invalid', now)).toBe('-');
	});

	it('uses next-match state for auction result input without rebuilding the API href', () => {
		const tournament: AuctionTournamentDto = {
			id: '01900000-0000-7000-8000-000000000601',
			displayNumber: 118,
			title: '20인 경매',
			format: 'TWENTY_PERSON',
			participantCount: 20,
			teamCount: 4,
			status: 'IN_GAME',
			revision: 7,
			activeMemberCount: 20,
			bidMemberCount: 16,
			incompleteMatchCount: 1,
			nextMatch: {
				id: '01900000-0000-7000-8000-000000000801',
				label: '결승',
				matchKey: 'FINAL',
				status: 'READY',
				confirmedGameCount: 1
			},
			championTeam: null,
			updatedAt: '2026-07-12T11:00:00Z',
			endedAt: null,
			href: '/g/200000000000000001/auction/tournaments/01900000-0000-7000-8000-000000000601/bracket?view=result'
		};

		const view = buildAuctionView({
			recruitments: [],
			activeTournaments: [tournament],
			completedTournaments: []
		});
		expect(view.activeWork[0]).toMatchObject({
			status: '결과 입력',
			detail: '결승 · 2세트 결과 대기',
			href: tournament.href
		});
	});
});
