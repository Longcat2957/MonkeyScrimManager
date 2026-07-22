import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const semifinalAMatchId = '01900000-0000-7000-8000-000000000701';
const semifinalBMatchId = '01900000-0000-7000-8000-000000000702';
const finalMatchId = '01900000-0000-7000-8000-000000000703';
const teamOneId = '01900000-0000-7000-8000-000000000711';
const teamTwoId = '01900000-0000-7000-8000-000000000712';
const teamThreeId = '01900000-0000-7000-8000-000000000713';
const teamFourId = '01900000-0000-7000-8000-000000000714';

function team(id: string, index: number) {
	return {
		id,
		teamIndex: index,
		teamName: `${index}팀`,
		initialPoints: 1000,
		currentPoints: 1000,
		members: []
	};
}

function bracketSnapshot(format: 'TEN_PERSON' | 'TWENTY_PERSON' = 'TWENTY_PERSON') {
	return {
		tournament: {
			id: tournamentId,
			title: '토요 20인 경매',
			displayNumber: 118,
			format,
			rules: {
				hardFearless: true,
				semifinalBestOf: format === 'TWENTY_PERSON' ? 3 : null,
				finalBestOf: 5
			},
			status: 'IN_GAME',
			revision: 12,
			href: `/g/${guildId}/auction/tournaments/${tournamentId}`
		},
		teams: [team(teamOneId, 1), team(teamTwoId, 2), team(teamThreeId, 3), team(teamFourId, 4)],
		bracket: {
			locked: true,
			slots: [
				{ slotKey: 'A_0', teamId: teamOneId },
				{ slotKey: 'A_1', teamId: teamTwoId },
				{ slotKey: 'B_0', teamId: teamThreeId },
				{ slotKey: 'B_1', teamId: teamFourId }
			],
			matches: [
				{
					id: semifinalAMatchId,
					matchKey: 'SEMIFINAL_A',
					bestOf: 3,
					hardFearless: true,
					status: 'LINEUP_PENDING',
					revision: 2,
					leftTeamId: teamOneId,
					rightTeamId: teamTwoId,
					leftScore: 0,
					rightScore: 0,
					winnerTeamId: null,
					lineupsLocked: false
				},
				{
					id: semifinalBMatchId,
					matchKey: 'SEMIFINAL_B',
					bestOf: 3,
					hardFearless: true,
					status: 'LINEUP_PENDING',
					revision: 2,
					leftTeamId: teamThreeId,
					rightTeamId: teamFourId,
					leftScore: 0,
					rightScore: 0,
					winnerTeamId: null,
					lineupsLocked: false
				},
				{
					id: finalMatchId,
					matchKey: 'FINAL',
					bestOf: 5,
					hardFearless: true,
					status: 'LINEUP_PENDING',
					revision: 0,
					leftTeamId: null,
					rightTeamId: null,
					leftScore: 0,
					rightScore: 0,
					winnerTeamId: null,
					lineupsLocked: false
				}
			]
		},
		permissions: {
			canManageBracket: true,
			canManageMatches: true,
			canRecoverBracket: false,
			canRevertDraft: false,
			canCancel: true
		}
	};
}

function gameSnapshot(
	matchId: string,
	matchKey: 'SEMIFINAL_A' | 'SEMIFINAL_B',
	leftTeamId: string,
	rightTeamId: string
) {
	return {
		match: {
			id: matchId,
			tournamentId,
			matchKey,
			label: matchKey === 'SEMIFINAL_A' ? '준결승 A' : '준결승 B',
			bestOf: 3,
			hardFearless: true,
			status: 'LINEUP_PENDING',
			revision: 2,
			leftTeamId,
			rightTeamId,
			leftScore: 0,
			rightScore: 0,
			winnerTeamId: null,
			lineupsLocked: false
		},
		teams: [
			{
				id: leftTeamId,
				teamName: '왼쪽 팀',
				members: [
					{
						teamMemberId: `${leftTeamId.slice(0, -1)}1`,
						userId: '1525847677542207501',
						displayName: '왼쪽 선수',
						declaredLane: 'TOP',
						mmrAtAcquisition: 1600,
						snapshotOrigin: 'CAPTURED',
						acquiredVia: 'CAPTAIN',
						rosterSlot: 1
					}
				]
			},
			{
				id: rightTeamId,
				teamName: '오른쪽 팀',
				members: [
					{
						teamMemberId: `${rightTeamId.slice(0, -1)}1`,
						userId: '1525847677542207502',
						displayName: '오른쪽 선수',
						declaredLane: 'MID',
						mmrAtAcquisition: 1580,
						snapshotOrigin: 'CAPTURED',
						acquiredVia: 'BID',
						rosterSlot: 1
					}
				]
			}
		],
		lineups: [],
		games: [],
		permissions: {
			canManageLineup: true,
			canEnterResults: false,
			canUnlockLineup: false,
			canRevertLastGame: false
		}
	};
}

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('auction tournament lineup page server load', () => {
	it('loads the locked 20-person bracket and both semifinal match snapshots in bracket order', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === `/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket`) {
				return envelope(bracketSnapshot());
			}
			if (path === `/api/guilds/${guildId}/auction-matches/${semifinalAMatchId}`) {
				return envelope(gameSnapshot(semifinalAMatchId, 'SEMIFINAL_A', teamOneId, teamTwoId));
			}
			if (path === `/api/guilds/${guildId}/auction-matches/${semifinalBMatchId}`) {
				return envelope(gameSnapshot(semifinalBMatchId, 'SEMIFINAL_B', teamThreeId, teamFourId));
			}
			return new Response('not found', { status: 404 });
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: tournamentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/lineup`)
		} as never);

		expect(parent).not.toHaveBeenCalled();
		expect(fetcher.mock.calls.map(([input]) => String(input))).toEqual([
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket`,
			`/api/guilds/${guildId}/auction-matches/${semifinalAMatchId}`,
			`/api/guilds/${guildId}/auction-matches/${semifinalBMatchId}`
		]);
		expect(result).toMatchObject({
			guildId,
			bracket: { tournament: { format: 'TWENTY_PERSON' }, bracket: { locked: true } },
			semifinalSnapshots: [
				{ match: { id: semifinalAMatchId, matchKey: 'SEMIFINAL_A' } },
				{ match: { id: semifinalBMatchId, matchKey: 'SEMIFINAL_B' } }
			]
		});
	});

	it('redirects an unfinished bidding tournament back to its draft', async () => {
		await expect(
			load({
				fetch: async () =>
					Response.json(
						{
							error: {
								code: 'auction_tournament_bracket_unavailable',
								message: '드래프트가 완료된 뒤 브래킷을 확인할 수 있습니다.',
								requestId
							}
						},
						{ status: 409 }
					),
				params: { guildId, id: tournamentId },
				parent: async () => ({ guildId }),
				url: new URL(
					`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/lineup`
				)
			} as never)
		).rejects.toMatchObject({
			status: 303,
			location: `/g/${guildId}/auction/tournaments/${tournamentId}/draft`
		});
	});

	it('redirects a ten-person tournament to its existing bracket workflow', async () => {
		await expect(
			load({
				fetch: async () => envelope(bracketSnapshot('TEN_PERSON')),
				params: { guildId, id: tournamentId },
				parent: async () => ({ guildId }),
				url: new URL(
					`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/lineup`
				)
			} as never)
		).rejects.toMatchObject({
			status: 303,
			location: `/g/${guildId}/auction/tournaments/${tournamentId}/bracket`
		});
	});
});
