import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const tournamentId = '01900000-0000-7000-8000-000000000601';

function snapshot() {
	return {
		tournament: {
			id: tournamentId,
			sourceRecruitmentId: recruitmentId,
			displayNumber: 42,
			title: '금요 경매 내전',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			status: 'BIDDING',
			revision: 7,
			href: `/g/${guildId}/auction/tournaments/${tournamentId}`
		},
		teams: [
			{
				id: '01900000-0000-7000-8000-000000000611',
				teamIndex: 1,
				teamName: '1팀',
				initialPoints: 1000,
				currentPoints: 1000,
				pointRevision: 0,
				members: [
					{
						teamMemberId: '01900000-0000-7000-8000-000000000621',
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
				id: '01900000-0000-7000-8000-000000000612',
				teamIndex: 2,
				teamName: '2팀',
				initialPoints: 1000,
				currentPoints: 1000,
				pointRevision: 0,
				members: []
			}
		],
		candidates: [
			{
				userId: '1525847677542207549',
				displayName: 'Nyan',
				declaredLane: 'JUNGLE',
				mmrAtStart: 1580,
				snapshotOrigin: 'CAPTURED',
				queuePosition: 0,
				state: 'QUEUED',
				drawCount: 0
			}
		],
		draft: { currentTarget: null, currentTargetProfile: null, unsold: [], recentAction: null },
		permissions: { canManageDraft: true, canManageBracket: false, canRevertLast: false }
	};
}

describe('auction tournament draft page server load', () => {
	it('loads the real tournament snapshot instead of a fixture route', async () => {
		const parent = vi.fn().mockResolvedValue({ guildId });
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot(), meta: { requestId } });
		});

		const result = await load({
			fetch: fetcher,
			params: { guildId, id: tournamentId },
			parent,
			url: new URL(`http://localhost:5173/g/${guildId}/auction/tournaments/${tournamentId}/draft`)
		} as never);

		expect(parent).toHaveBeenCalledOnce();
		expect(String(fetcher.mock.calls[0]?.[0])).toBe(
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}`
		);
		expect(result).toMatchObject({
			guildId,
			snapshot: {
				tournament: { id: tournamentId, revision: 7 },
				permissions: { canManageDraft: true, canManageBracket: false, canRevertLast: false }
			}
		});
	});
});
