import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	auctionTournamentBracketErrorText,
	executeAuctionTournamentBracketCommand,
	fetchAuctionTournamentBracket,
	isAuctionTournamentBracketRevisionConflict
} from './auctionTournamentBracket';

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
			status: 'BIDDING',
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
		permissions: { canManageBracket: true, canManageMatches: true, canRecoverBracket: false }
	};
}

function installBrowser(fetcher: typeof globalThis.fetch) {
	vi.stubGlobal('fetch', fetcher);
	vi.stubGlobal('document', { cookie: 'theme=dark; monkeybot_csrf=csrf-token' });
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('auction tournament bracket API', () => {
	it('loads the authoritative bracket snapshot from its dedicated endpoint', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot(), meta: { requestId } });
		});
		installBrowser(fetcher);

		const result = await fetchAuctionTournamentBracket(guildId, tournamentId);

		expect(result.tournament.id).toBe(tournamentId);
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket`);
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('sends an optimistic revision, CSRF token, and idempotency key for bracket commands', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot(), meta: { requestId } });
		});
		installBrowser(fetcher);
		const idempotencyKey = '01900000-0000-7000-8000-000000000002';

		await executeAuctionTournamentBracketCommand(
			guildId,
			tournamentId,
			{ kind: 'ASSIGN_TEAM', expectedRevision: 7, slotKey: 'A_1', teamId: teamTwoId },
			idempotencyKey
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket/commands`
		);
		expect(init?.method).toBe('POST');
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		expect(JSON.parse(String(init?.body))).toEqual({
			kind: 'ASSIGN_TEAM',
			expectedRevision: 7,
			slotKey: 'A_1',
			teamId: teamTwoId
		});
	});

	it('sends an unlock command through the same versioned bracket command endpoint', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot(), meta: { requestId } });
		});
		installBrowser(fetcher);
		const idempotencyKey = '01900000-0000-7000-8000-000000000002';

		await executeAuctionTournamentBracketCommand(
			guildId,
			tournamentId,
			{ kind: 'UNLOCK_BRACKET', expectedRevision: 7 },
			idempotencyKey
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(
			`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/bracket/commands`
		);
		expect(init?.method).toBe('POST');
		expect(JSON.parse(String(init?.body))).toEqual({
			kind: 'UNLOCK_BRACKET',
			expectedRevision: 7
		});
	});

	it('preserves the tournament revision conflict code and request ID', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_tournament_revision_conflict',
						message: '다른 운영자가 경매 브래킷을 변경했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await executeAuctionTournamentBracketCommand(
			guildId,
			tournamentId,
			{ kind: 'UNLOCK_BRACKET', expectedRevision: 7 },
			'01900000-0000-7000-8000-000000000003'
		).catch((cause) => cause);

		expect(isAuctionTournamentBracketRevisionConflict(error)).toBe(true);
		expect(auctionTournamentBracketErrorText(error)).toBe(
			`다른 운영자가 경매 브래킷을 변경했습니다. (요청 ID: ${requestId})`
		);
	});

	it('does not classify a non-revision bracket conflict as a stale snapshot', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_tournament_bracket_locked',
						message: '경기 기록이 있어 브래킷을 변경할 수 없습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await executeAuctionTournamentBracketCommand(
			guildId,
			tournamentId,
			{ kind: 'UNLOCK_BRACKET', expectedRevision: 7 },
			'01900000-0000-7000-8000-000000000004'
		).catch((cause) => cause);

		expect(isAuctionTournamentBracketRevisionConflict(error)).toBe(false);
		expect(auctionTournamentBracketErrorText(error)).toBe(
			`경기 기록이 있어 브래킷을 변경할 수 없습니다. (요청 ID: ${requestId})`
		);
	});
});
