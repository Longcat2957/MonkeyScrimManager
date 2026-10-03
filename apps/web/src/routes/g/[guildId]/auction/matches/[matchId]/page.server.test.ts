import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const matchId = '01900000-0000-7000-8000-000000000701';
const semifinalBMatchId = '01900000-0000-7000-8000-000000000702';
const leftTeamId = '01900000-0000-7000-8000-000000000711';
const rightTeamId = '01900000-0000-7000-8000-000000000712';
const teamThreeId = '01900000-0000-7000-8000-000000000713';
const teamFourId = '01900000-0000-7000-8000-000000000714';

function snapshot() {
	return {
		match: {
			id: matchId,
			tournamentId,
			matchKey: 'FINAL',
			label: '결승',
			bestOf: 3,
			hardFearless: true,
			status: 'LINEUP_PENDING',
			revision: 7,
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
				teamName: '1팀',
				members: [
					{
						teamMemberId: '01900000-0000-7000-8000-000000000721',
						userId: '1525847677542207548',
						displayName: 'Longcat',
						declaredLane: 'MID',
						mmrAtAcquisition: 1623,
						snapshotOrigin: 'CAPTURED',
						acquiredVia: 'CAPTAIN',
						rosterSlot: 1
					}
				]
			},
			{
				id: rightTeamId,
				teamName: '2팀',
				members: [
					{
						teamMemberId: '01900000-0000-7000-8000-000000000722',
						userId: '1525847677542207549',
						displayName: 'Nyan',
						declaredLane: 'TOP',
						mmrAtAcquisition: 1588,
						snapshotOrigin: 'CAPTURED',
						acquiredVia: 'CAPTAIN',
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
			canRevertLastGame: false,
			canCompleteFinalAsBo1: false
		}
	};
}

const championCatalogResponse = {
	type: 'champion',
	format: 'standAloneComplex',
	version: '16.13.1',
	data: {
		Aatrox: {
			version: '16.13.1',
			id: 'Aatrox',
			key: '266',
			name: '아트록스',
			title: '다르킨의 검',
			image: { full: 'Aatrox.png' },
			tags: ['Fighter']
		}
	}
};

function twentyPersonBracketWithPendingSemifinal() {
	const team = (id: string, teamIndex: number) => ({
		id,
		teamIndex,
		teamName: `${teamIndex}팀`,
		initialPoints: 1000,
		currentPoints: 1000,
		members: []
	});
	return {
		tournament: {
			id: tournamentId,
			title: '토요 20인 경매',
			displayNumber: 42,
			format: 'TWENTY_PERSON',
			rules: { hardFearless: true, semifinalBestOf: 3, finalBestOf: 5 },
			status: 'IN_GAME',
			revision: 8,
			href: `/g/${guildId}/auction/tournaments/${tournamentId}`
		},
		teams: [team(leftTeamId, 1), team(rightTeamId, 2), team(teamThreeId, 3), team(teamFourId, 4)],
		bracket: {
			locked: true,
			slots: [
				{ slotKey: 'A_0', teamId: leftTeamId },
				{ slotKey: 'A_1', teamId: rightTeamId },
				{ slotKey: 'B_0', teamId: teamThreeId },
				{ slotKey: 'B_1', teamId: teamFourId }
			],
			matches: [
				{
					id: matchId,
					matchKey: 'SEMIFINAL_A',
					bestOf: 3,
					hardFearless: true,
					leftTeamId,
					rightTeamId,
					status: 'READY',
					revision: 8,
					leftScore: 0,
					rightScore: 0,
					winnerTeamId: null,
					lineupsLocked: true
				},
				{
					id: semifinalBMatchId,
					matchKey: 'SEMIFINAL_B',
					bestOf: 3,
					hardFearless: true,
					leftTeamId: teamThreeId,
					rightTeamId: teamFourId,
					status: 'LINEUP_PENDING',
					revision: 2,
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

describe('auction match result page server load', () => {
	it('loads the canonical game snapshot and the champion catalog', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === `/api/guilds/${guildId}/auction-matches/${matchId}`) {
				return Response.json({ data: snapshot(), meta: { requestId } });
			}
			if (path === '/dd/api/versions.json') {
				return new Response('unavailable', { status: 503 });
			}
			if (path === '/fixtures/datadragon/champion-ko_KR.fixture.json') {
				return Response.json(championCatalogResponse);
			}
			return new Response('not found', { status: 404 });
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, matchId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/matches/${matchId}`)
		} as never);

		expect(parent).not.toHaveBeenCalled();
		expect(fetcher.mock.calls.map(([path]) => path)).toContain(
			`/api/guilds/${guildId}/auction-matches/${matchId}`
		);
		expect(fetcher.mock.calls.map(([path]) => path)).toContain(
			'/fixtures/datadragon/champion-ko_KR.fixture.json'
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: { match: { id: matchId, tournamentId, revision: 7 }, games: [] },
			championCatalog: { version: '16.13.1', byId: { Aatrox: { name: '아트록스' } } }
		});
	});

	it('keeps a 20-person semifinal in the shared lineup stage until both lineups lock', async () => {
		const semifinalSnapshot = snapshot();
		semifinalSnapshot.match = {
			...semifinalSnapshot.match,
			matchKey: 'SEMIFINAL_A',
			label: '준결승 A',
			status: 'READY'
		};
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === `/api/guilds/${guildId}/auction-matches/${matchId}`) {
				return Response.json({ data: semifinalSnapshot, meta: { requestId } });
			}
			if (path === `/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket`) {
				return Response.json({
					data: twentyPersonBracketWithPendingSemifinal(),
					meta: { requestId }
				});
			}
			return new Response('not found', { status: 404 });
		});

		await expect(
			load({
				fetch: fetcher,
				params: { guildId, matchId },
				parent: async () => ({ guildId }),
				url: new URL(`http://localhost:5173/g/${guildId}/auction/matches/${matchId}`)
			} as never)
		).rejects.toMatchObject({
			status: 303,
			location: `/g/${guildId}/auction/tournaments/${tournamentId}/lineup`
		});
	});

	it('keeps an early-finished semifinal readable without redirecting to lineup setup', async () => {
		const finished = snapshot();
		finished.match = { ...finished.match, matchKey: 'SEMIFINAL_A', status: 'COMPLETED' };
		const bracket = twentyPersonBracketWithPendingSemifinal();
		bracket.tournament.status = 'COMPLETED';
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === `/api/guilds/${guildId}/auction-matches/${matchId}`) {
				return Response.json({
					data: { ...finished, match: { ...finished.match, endedEarly: true } },
					meta: { requestId }
				});
			}
			if (path.endsWith('/bracket')) return Response.json({ data: bracket, meta: { requestId } });
			if (path.endsWith('champion-ko_KR.fixture.json'))
				return Response.json(championCatalogResponse);
			return new Response('unavailable', { status: 503 });
		});
		await expect(
			load({
				fetch: fetcher,
				params: { guildId, matchId },
				url: new URL(`http://localhost:5173/g/${guildId}/auction/matches/${matchId}`)
			} as never)
		).resolves.toMatchObject({ snapshot: { match: { endedEarly: true, status: 'COMPLETED' } } });
	});
});
