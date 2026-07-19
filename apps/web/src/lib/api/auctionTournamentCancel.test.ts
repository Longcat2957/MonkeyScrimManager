import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	auctionTournamentCancelErrorText,
	cancelAuctionTournament,
	fetchAuctionTournamentCancelPreview,
	isAuctionTournamentCancelRevisionConflict
} from './auctionTournamentCancel';

const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';

const preview = {
	tournament: {
		id: tournamentId,
		displayNumber: 42,
		title: '금요 경매 내전',
		status: 'IN_GAME' as const,
		revision: 7
	}
};

const result = {
	tournamentId,
	tournamentStatus: 'CANCELLED' as const,
	tournamentRevision: 8,
	sourceRecruitmentId: recruitmentId,
	sourceRecruitmentRevision: 4,
	reversedGameCount: 2
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction tournament cancel API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads cancel preview from the existing tournament preview route', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return envelope(preview);
			}
		);
		installBrowser(fetcher);

		await expect(fetchAuctionTournamentCancelPreview(guildId, tournamentId)).resolves.toEqual(
			preview
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/cancel-preview`
		);
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('cancels with preview revision, reason, CSRF, and idempotency key', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return envelope(result);
			}
		);
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			cancelAuctionTournament(
				guildId,
				tournamentId,
				{ expectedRevision: 7, reason: '운영자 요청으로 조기 종료합니다.' },
				idempotencyKey
			)
		).resolves.toEqual(result);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/cancel`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 7,
			reason: '운영자 요청으로 조기 종료합니다.'
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('preserves backend conflict details for UI recovery text', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return Response.json(
					{
						error: {
							code: 'auction_tournament_revision_conflict',
							message: '다른 운영자가 경매 내전을 변경했습니다.',
							requestId
						}
					},
					{ status: 409 }
				);
			}
		);
		installBrowser(fetcher);

		const error = await cancelAuctionTournament(
			guildId,
			tournamentId,
			{ expectedRevision: 6, reason: '운영자 요청으로 조기 종료합니다.' },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionTournamentCancelRevisionConflict(error)).toBe(true);
		expect(auctionTournamentCancelErrorText(error)).toBe(
			`다른 운영자가 경매 내전을 변경했습니다. (요청 ID: ${requestId})`
		);
	});
});
