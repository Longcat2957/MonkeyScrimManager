import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const tournamentId = '01900000-0000-7000-8000-000000000601';

function snapshot(status: 'OPEN' | 'CONVERTED') {
	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 42,
			title: '수요 10인 경매',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			participantCount: 10,
			joinedParticipantCount: 1,
			teamCount: 2,
			status,
			revision: 0,
			channelId: '200000000000000002',
			messageId: '200000000000000003',
			createdById: '200000000000000004',
			updatedAt: '2026-07-14T12:00:00Z',
			href:
				status === 'CONVERTED'
					? `/g/${guildId}/auction/tournaments/${tournamentId}/draft`
					: `/g/${guildId}/auction/recruitments/${recruitmentId}`
		},
		participants: [{ id: '200000000000000004', displayName: 'Longcat', declaredLane: 'MID' }],
		links: { draft: null },
		permissions: {
			canManageMembers: false,
			canClose: false,
			canReopen: false,
			canRestore: false,
			canSendParticipantConfirmationDms: false,
			canCancel: true
		}
	};
}

describe('auction recruitment page server load', () => {
	it('loads the authoritative auction recruitment snapshot before setup begins', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			void input;
			return Response.json({
				data: snapshot('OPEN'),
				meta: { requestId }
			});
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: recruitmentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/recruitments/${recruitmentId}`)
		} as never);

		expect(parent).not.toHaveBeenCalled();
		expect(String(fetcher.mock.calls[0]?.[0])).toBe(
			`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}`
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: {
				recruitment: { id: recruitmentId, status: 'OPEN' },
				participants: [{ displayName: 'Longcat', declaredLane: 'MID' }]
			}
		});
	});

	it('redirects a converted recruitment detail URL to the linked auction tournament', async () => {
		await expect(
			load({
				fetch: async () => Response.json({ data: snapshot('CONVERTED'), meta: { requestId } }),
				params: { guildId, id: recruitmentId },
				parent: async () => ({ guildId }),
				url: new URL(`http://localhost:5173/g/${guildId}/auction/recruitments/${recruitmentId}`)
			} as never)
		).rejects.toMatchObject({
			status: 303,
			location: `/g/${guildId}/auction/tournaments/${tournamentId}/draft`
		});
	});
});
