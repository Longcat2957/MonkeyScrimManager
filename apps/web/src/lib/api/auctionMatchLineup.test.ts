import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	auctionMatchLineupSnapshotSchema,
	type AuctionMatchLineupSnapshot
} from '$lib/contracts/auctionMatchLineup';
import {
	auctionMatchLineupErrorText,
	assignAuctionMatchLineup,
	fetchAuctionMatchLineup,
	isAuctionMatchLineupRevisionConflict,
	lockAuctionMatchLineup,
	unlockAuctionMatchLineup
} from './auctionMatchLineup';

const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const matchId = '01900000-0000-7000-8000-000000000701';
const leftTeamId = '01900000-0000-7000-8000-000000000711';
const rightTeamId = '01900000-0000-7000-8000-000000000712';
const leftMemberId = '01900000-0000-7000-8000-000000000721';
const rightMemberId = '01900000-0000-7000-8000-000000000731';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const lanes = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;

const leftMembers = lanes.map((declaredLane, index) => ({
	teamMemberId: index === 0 ? leftMemberId : `01900000-0000-7000-8000-00000000072${index + 1}`,
	userId: `15258476775422075${String(index + 1).padStart(2, '0')}`,
	displayName: `1팀 선수 ${index + 1}`,
	declaredLane,
	mmrAtAcquisition: 1600 + index,
	snapshotOrigin: 'CAPTURED' as const,
	acquiredVia: 'CAPTAIN' as const,
	rosterSlot: index + 1
}));
const rightMembers = lanes.map((declaredLane, index) => ({
	teamMemberId: index === 0 ? rightMemberId : `01900000-0000-7000-8000-00000000073${index + 1}`,
	userId: `15258476775422076${String(index + 1).padStart(2, '0')}`,
	displayName: `2팀 선수 ${index + 1}`,
	declaredLane,
	mmrAtAcquisition: 1580 + index,
	snapshotOrigin: 'CAPTURED' as const,
	acquiredVia: 'CAPTAIN' as const,
	rosterSlot: index + 1
}));

const snapshot: AuctionMatchLineupSnapshot = {
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
			members: leftMembers
		},
		{
			id: rightTeamId,
			teamName: '2팀',
			members: rightMembers
		}
	],
	lineups: [],
	permissions: {
		canManageLineup: true,
		canEnterResults: false,
		canUnlockLineup: false,
		canRevertLastGame: false,
		canCompleteFinalAsBo1: false
	}
};

function lockedSnapshot(): AuctionMatchLineupSnapshot {
	return {
		...snapshot,
		match: { ...snapshot.match, status: 'READY', revision: 8, lineupsLocked: true },
		lineups: snapshot.teams.flatMap((team) =>
			team.members.map((member, index) => ({
				teamId: team.id,
				lane: lanes[index]!,
				teamMemberId: member.teamMemberId,
				userId: member.userId
			}))
		)
	};
}

function envelope(data: AuctionMatchLineupSnapshot = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction match lineup API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads a selected match lineup through its canonical route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope();
		});
		installBrowser(fetcher);

		await expect(fetchAuctionMatchLineup(guildId, matchId)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
	});

	it('accepts canonical match snapshots extended with confirmed game data', async () => {
		const canonicalSnapshot = { ...snapshot, games: [] };
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(canonicalSnapshot);
		});
		installBrowser(fetcher);

		await expect(fetchAuctionMatchLineup(guildId, matchId)).resolves.toEqual(canonicalSnapshot);
	});

	it('assigns one lineup slot with the server revision, CSRF, and an idempotency key', async () => {
		const nextSnapshot: AuctionMatchLineupSnapshot = {
			...snapshot,
			match: { ...snapshot.match, revision: 8 },
			lineups: [
				{
					teamId: leftTeamId,
					lane: 'MID',
					teamMemberId: leftMemberId,
					userId: leftMembers[0]!.userId
				}
			]
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(nextSnapshot);
		});
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			assignAuctionMatchLineup(
				guildId,
				matchId,
				{
					expectedRevision: 7,
					teamId: leftTeamId,
					lane: 'MID',
					teamMemberId: leftMemberId
				},
				idempotencyKey
			)
		).resolves.toEqual(nextSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}/lineup`);
		expect(init).toMatchObject({ method: 'PATCH', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 7,
			teamId: leftTeamId,
			lane: 'MID',
			teamMemberId: leftMemberId
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('locks a complete lineup with the match revision', async () => {
		const complete = lockedSnapshot();
		const parsed = auctionMatchLineupSnapshotSchema.safeParse(complete);
		if (!parsed.success) throw parsed.error;
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(complete);
		});
		installBrowser(fetcher);

		await lockAuctionMatchLineup(guildId, matchId, { expectedRevision: 7 }, idempotencyKey);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}/lineup/lock`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({ expectedRevision: 7 });
	});

	it('unlocks a confirmed lineup with the match revision, CSRF, and an idempotency key', async () => {
		const complete = lockedSnapshot();
		const nextSnapshot: AuctionMatchLineupSnapshot = {
			...snapshot,
			match: { ...snapshot.match, revision: 9 }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(nextSnapshot);
		});
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			unlockAuctionMatchLineup(
				guildId,
				matchId,
				{ expectedRevision: complete.match.revision },
				idempotencyKey
			)
		).resolves.toEqual(nextSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}/lineup/unlock`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({ expectedRevision: 8 });
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('preserves a revision conflict reason and request ID for the caller', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_match_revision_conflict',
						message: '다른 운영자가 먼저 라인업을 변경했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await lockAuctionMatchLineup(
			guildId,
			matchId,
			{ expectedRevision: 7 },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionMatchLineupRevisionConflict(error)).toBe(true);
		expect(auctionMatchLineupErrorText(error)).toBe(
			`다른 운영자가 먼저 라인업을 변경했습니다. (요청 ID: ${requestId})`
		);
	});

	it('does not classify a non-revision conflict as a revision conflict', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_match_lineup_incomplete',
						message: '라인업을 모두 배치해야 합니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await lockAuctionMatchLineup(
			guildId,
			matchId,
			{ expectedRevision: 7 },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionMatchLineupRevisionConflict(error)).toBe(false);
		expect(auctionMatchLineupErrorText(error)).toBe(
			`라인업을 모두 배치해야 합니다. (요청 ID: ${requestId})`
		);
	});
});
