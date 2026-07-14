import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const matchId = '01900000-0000-7000-8000-000000000701';
const leftTeamId = '01900000-0000-7000-8000-000000000711';
const rightTeamId = '01900000-0000-7000-8000-000000000712';

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
		canRevertLastGame: false
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

		expect(parent).toHaveBeenCalledOnce();
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
});
