import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const teamOneId = '01900000-0000-7000-8000-000000000611';
const teamTwoId = '01900000-0000-7000-8000-000000000612';

function snapshot(
	status: 'BRACKET_SETUP' | 'IN_GAME' | 'COMPLETED' | 'CANCELLED' = 'BRACKET_SETUP'
) {
	return {
		tournament: {
			id: tournamentId,
			title: '금요 경매 내전',
			displayNumber: 42,
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			status,
			revision: 7,
			href: `/g/${guildId}/auction/tournaments/${tournamentId}`
		},
		teams: [
			{
				id: teamOneId,
				teamIndex: 1,
				teamName: '1팀',
				initialPoints: 1000,
				currentPoints: 1000,
				members: []
			},
			{
				id: teamTwoId,
				teamIndex: 2,
				teamName: '2팀',
				initialPoints: 1000,
				currentPoints: 1000,
				members: []
			}
		],
		bracket: {
			locked: false,
			slots: [
				{ slotKey: 'A_0', teamId: teamOneId },
				{ slotKey: 'A_1', teamId: null },
				{ slotKey: 'B_0', teamId: null },
				{ slotKey: 'B_1', teamId: null }
			],
			matches: []
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

describe('auction tournament bracket page server load', () => {
	it('loads the dedicated backend bracket snapshot without fixture routing', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot(), meta: { requestId } });
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: tournamentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/bracket`)
		} as never);

		expect(parent).not.toHaveBeenCalled();
		expect(fetcher.mock.calls[0]?.[0]).toBe(
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket`
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: { tournament: { id: tournamentId, revision: 7 }, bracket: { locked: false } }
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
					`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/bracket`
				)
			} as never)
		).rejects.toMatchObject({
			status: 303,
			location: `/g/${guildId}/auction/tournaments/${tournamentId}/draft`
		});
	});

	it.each(['IN_GAME', 'COMPLETED', 'CANCELLED'] as const)(
		'redirects %s tournaments to the read-only result bracket',
		async (status) => {
			await expect(
				load({
					fetch: async () => Response.json({ data: snapshot(status), meta: { requestId } }),
					params: { guildId, id: tournamentId },
					parent: async () => ({ guildId }),
					url: new URL(
						`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/bracket`
					)
				} as never)
			).rejects.toMatchObject({
				status: 303,
				location: `/g/${guildId}/auction/tournaments/${tournamentId}/result`
			});
		}
	);
});
