import { describe, expect, it } from 'vitest';
import { createMockBracketMatches, findMockAuctionTournament } from '$lib/fixtures/auctionWorkflow';
import { mockAuctionBracketTeams10 } from '$lib/fixtures/workflow';
import { scoreAuctionGames } from '../auction';

describe('auction workflow contracts', () => {
	it('uses tournament data instead of inferring the format at the page', () => {
		expect(findMockAuctionTournament('a10-117')?.teamCount).toBe(2);
		expect(findMockAuctionTournament('a20-117')?.teamCount).toBe(4);
		expect(findMockAuctionTournament('missing')).toBeNull();
	});

	it('maps bracket lineups by each member role and id', () => {
		const [match] = createMockBracketMatches(mockAuctionBracketTeams10);
		const leftTeam = mockAuctionBracketTeams10[0]!;
		const top = leftTeam.members.find((member) => member.role === 'TOP')!;

		expect(match?.lineups[leftTeam.id]?.TOP).toBe(top.id);
	});

	it('scores swapped blue and red sides against the correct team', () => {
		const games = [
			{
				id: 'set-1',
				label: '1세트',
				sidePreset: 'DEFAULT' as const,
				winner: 'BLUE' as const,
				bans: { BLUE: [], RED: [] },
				picks: {}
			},
			{
				id: 'set-2',
				label: '2세트',
				sidePreset: 'SWAPPED' as const,
				winner: 'RED' as const,
				bans: { BLUE: [], RED: [] },
				picks: {}
			},
			{
				id: 'set-3',
				label: '3세트',
				sidePreset: 'SWAPPED' as const,
				winner: 'BLUE' as const,
				bans: { BLUE: [], RED: [] },
				picks: {}
			}
		];

		expect(scoreAuctionGames(games)).toEqual({ leftScore: 2, rightScore: 1 });
	});
});
