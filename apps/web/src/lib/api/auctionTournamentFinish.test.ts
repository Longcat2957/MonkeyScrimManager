import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	auctionTournamentFinishErrorText,
	finishAuctionTournament,
	fetchAuctionTournamentFinishPreview,
	isAuctionTournamentFinishRevisionConflict
} from './auctionTournamentFinish';

const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
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
	tournamentStatus: 'COMPLETED' as const,
	tournamentRevision: 8,
	championTeamId: null
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction tournament finish API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads finish preview from the tournament finish preview route', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return envelope(preview);
			}
		);
		installBrowser(fetcher);

		await expect(fetchAuctionTournamentFinishPreview(guildId, tournamentId)).resolves.toEqual(
			preview
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/finish-preview`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('finishs with preview revision, reason, CSRF, and idempotency key', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return envelope(result);
			}
		);
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			finishAuctionTournament(
				guildId,
				tournamentId,
				{ expectedRevision: 7, reason: '운영자 요청으로 마무리합니다.' },
				idempotencyKey
			)
		).resolves.toEqual(result);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/finish`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 7,
			reason: '운영자 요청으로 마무리합니다.'
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('preserves permission failure details and request ID', async () => {
		installBrowser(
			vi.fn(async () =>
				Response.json(
					{
						error: {
							code: 'auction_manager_required',
							message: '경매 운영자 권한이 필요합니다.',
							requestId
						}
					},
					{ status: 403 }
				)
			)
		);
		const error = await finishAuctionTournament(
			guildId,
			tournamentId,
			{ expectedRevision: 7, reason: '참가자 요청' },
			idempotencyKey
		).catch((cause) => cause);
		expect(error.code).toBe('auction_manager_required');
		expect(auctionTournamentFinishErrorText(error)).toBe(
			`경매 운영자 권한이 필요합니다. (요청 ID: ${requestId})`
		);
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

		const error = await finishAuctionTournament(
			guildId,
			tournamentId,
			{ expectedRevision: 6, reason: '운영자 요청으로 마무리합니다.' },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionTournamentFinishRevisionConflict(error)).toBe(true);
		expect(auctionTournamentFinishErrorText(error)).toBe(
			`다른 운영자가 경매 내전을 변경했습니다. (요청 ID: ${requestId})`
		);
	});
});
