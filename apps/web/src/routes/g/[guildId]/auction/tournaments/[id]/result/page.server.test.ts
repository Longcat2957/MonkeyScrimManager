import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const teamOneId = '01900000-0000-7000-8000-000000000611';
const teamTwoId = '01900000-0000-7000-8000-000000000612';

function snapshot() {
	return {
		tournament: {
			id: tournamentId,
			title: '금요 경매 내전',
			displayNumber: 42,
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			status: 'IN_GAME',
			revision: 7,
			championTeamId: null,
			href: `/g/${guildId}/auction/tournaments/${tournamentId}`
		},
		teams: [
			{
				id: teamOneId,
				teamIndex: 1,
				teamName: '1팀',
				initialPoints: 1000,
				currentPoints: 720,
				spentPoints: 280,
				members: []
			},
			{
				id: teamTwoId,
				teamIndex: 2,
				teamName: '2팀',
				initialPoints: 1000,
				currentPoints: 710,
				spentPoints: 290,
				members: []
			}
		],
		matches: [],
		players: [],
		permissions: { canRevertResults: true }
	};
}

describe('auction tournament result page server load', () => {
	it('loads the dedicated authoritative result endpoint without fixture routing', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot(), meta: { requestId } });
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: tournamentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/result`)
		} as never);

		expect(parent).toHaveBeenCalledOnce();
		expect(fetcher.mock.calls[0]?.[0]).toBe(
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/result`
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: { tournament: { id: tournamentId, championTeamId: null }, players: [] }
		});
	});
});
