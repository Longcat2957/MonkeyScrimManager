import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	auctionTournamentErrorText,
	createAuctionTournament,
	fetchAuctionSetupDraft,
	saveAuctionSetupDraft
} from './auctionTournament';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';

const setupSnapshot = {
	draft: {
		recruitmentId,
		revision: 4,
		teamCount: 2 as const,
		teams: [
			{ teamIndex: 1, teamName: '1팀', captainUserId: '1525847677542207548', initialPoints: 1000 },
			{ teamIndex: 2, teamName: '2팀', captainUserId: '1525847677542207549', initialPoints: 1200 }
		]
	},
	participants: [
		{ id: '1525847677542207548', displayName: 'Longcat', declaredLane: 'MID' as const },
		{ id: '1525847677542207549', displayName: 'Nyan', declaredLane: 'TOP' as const }
	],
	permissions: { canEdit: true, canCreateTournament: true }
};

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction tournament setup API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the persisted setup draft through the same-origin API route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: setupSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(fetchAuctionSetupDraft(guildId, recruitmentId)).resolves.toEqual(setupSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/setup-draft`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
	});

	it('saves complete captain and point settings with CSRF and an idempotency key', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: setupSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			saveAuctionSetupDraft(guildId, recruitmentId, {
				expectedRevision: 4,
				teams: setupSnapshot.draft.teams,
				idempotencyKey
			})
		).resolves.toEqual(setupSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/setup-draft`);
		expect(init).toMatchObject({ method: 'PATCH', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 4,
			teams: setupSnapshot.draft.teams
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('creates a tournament from the saved setup revision and returns its draft href', async () => {
		const href = `/g/${guildId}/auction/tournaments/${tournamentId}/draft`;
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({
				data: { tournament: { id: tournamentId, href, status: 'BIDDING' } },
				meta: { requestId }
			});
		});
		installBrowser(fetcher);

		await expect(
			createAuctionTournament(guildId, {
				sourceRecruitmentId: recruitmentId,
				expectedRevision: 4,
				idempotencyKey
			})
		).resolves.toMatchObject({ id: tournamentId, href });

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			sourceRecruitmentId: recruitmentId,
			expectedRevision: 4
		});
	});

	it('preserves a backend setup failure and request ID for the UI', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_setup_draft_revision_conflict',
						message: '다른 운영자가 경매 준비 설정을 변경했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await saveAuctionSetupDraft(guildId, recruitmentId, {
			expectedRevision: 4,
			teams: setupSnapshot.draft.teams,
			idempotencyKey
		}).catch((cause) => cause);

		expect(error).toMatchObject({
			status: 409,
			code: 'auction_setup_draft_revision_conflict',
			requestId
		});
		expect(auctionTournamentErrorText(error)).toBe(
			`다른 운영자가 경매 준비 설정을 변경했습니다. (요청 ID: ${requestId})`
		);
	});
});
