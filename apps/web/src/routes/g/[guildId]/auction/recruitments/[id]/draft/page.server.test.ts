import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000501';

function snapshot(status: 'OPEN' | 'CLOSED') {
	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 42,
			title: '수요 10인 경매',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			participantCount: 10,
			joinedParticipantCount: 10,
			teamCount: 2,
			status,
			revision: 1,
			channelId: '200000000000000002',
			messageId: '200000000000000003',
			createdById: '200000000000000004',
			updatedAt: '2026-07-14T12:00:00Z',
			href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
		},
		participants: [{ id: '200000000000000004', displayName: 'Longcat', declaredLane: 'MID' }],
		links: {
			draft:
				status === 'CLOSED' ? `/g/${guildId}/auction/recruitments/${recruitmentId}/draft` : null
		},
		permissions: {
			canManageMembers: false,
			canClose: false,
			canSendParticipantConfirmationDms: true,
			canCancel: false
		}
	};
}

function setupSnapshot() {
	return {
		draft: {
			recruitmentId,
			revision: 4,
			teamCount: 2,
			teams: [
				{ teamIndex: 1, teamName: '1팀', captainUserId: null, initialPoints: 1000 },
				{ teamIndex: 2, teamName: '2팀', captainUserId: null, initialPoints: 1000 }
			]
		},
		participants: [{ id: '200000000000000004', displayName: 'Longcat', declaredLane: 'MID' }],
		permissions: { canEdit: true, canCreateTournament: true }
	};
}

describe('auction recruitment closed-roster page server load', () => {
	it('loads the real closed recruitment snapshot instead of a fixture route', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			const data = path.endsWith('/setup-draft') ? setupSnapshot() : snapshot('CLOSED');
			return Response.json({ data, meta: { requestId } });
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: recruitmentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/recruitments/${recruitmentId}/draft`)
		} as never);

		expect(parent).not.toHaveBeenCalled();
		expect(String(fetcher.mock.calls[0]?.[0])).toBe(
			`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}`
		);
		expect(String(fetcher.mock.calls[1]?.[0])).toBe(
			`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/setup-draft`
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: { recruitment: { id: recruitmentId, status: 'CLOSED' } },
			setup: { draft: { recruitmentId, revision: 4 } }
		});
	});

	it('redirects an open recruitment back to its active recruitment page', async () => {
		await expect(
			load({
				fetch: async () => Response.json({ data: snapshot('OPEN'), meta: { requestId } }),
				params: { guildId, id: recruitmentId },
				parent: async () => ({ guildId }),
				url: new URL(
					`http://localhost:5173/g/${guildId}/auction/recruitments/${recruitmentId}/draft`
				)
			} as never)
		).rejects.toMatchObject({
			status: 303,
			location: `/g/${guildId}/auction/recruitments/${recruitmentId}`
		});
	});
});
