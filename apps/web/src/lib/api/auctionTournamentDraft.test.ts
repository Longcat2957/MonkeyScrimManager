import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AuctionTournamentDraftSnapshot } from '$lib/contracts/auctionTournamentDraft';
import {
	auctionTournamentDraftErrorText,
	executeAuctionTournamentDraftCommand,
	fetchAuctionTournamentDraft,
	isAuctionTournamentDraftRevisionConflict
} from './auctionTournamentDraft';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const teamOneId = '01900000-0000-7000-8000-000000000611';
const teamTwoId = '01900000-0000-7000-8000-000000000612';
const teamMemberId = '01900000-0000-7000-8000-000000000621';
const activeUserId = '1525847677542207548';
const queuedUserId = '1525847677542207549';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';

const activeCandidate = {
	userId: activeUserId,
	displayName: 'Longcat',
	declaredLane: 'MID' as const,
	mmrAtStart: 1623,
	snapshotOrigin: 'CAPTURED' as const,
	queuePosition: 0,
	state: 'ACTIVE' as const,
	drawCount: 1
};

const activeCandidateProfile = {
	avatarUrl: null,
	profileIconId: 588,
	riotTier: 'P3',
	mainLane: 'MID' as const,
	summary: { games: 12, wins: 7, losses: 5 },
	laneMmr: [
		{ lane: 'TOP' as const, mmr: 1500, games: 0, wins: 0 },
		{ lane: 'JUNGLE' as const, mmr: 1500, games: 0, wins: 0 },
		{ lane: 'MID' as const, mmr: 1623, games: 12, wins: 7 },
		{ lane: 'BOTTOM' as const, mmr: 1500, games: 0, wins: 0 },
		{ lane: 'SUPPORT' as const, mmr: 1500, games: 0, wins: 0 }
	],
	championIds: ['Ahri', 'Orianna', 'Syndra'],
	championRecords: [
		{ championId: 'Ahri', games: 6, wins: 4, losses: 2 },
		{ championId: 'Orianna', games: 4, wins: 2, losses: 2 },
		{ championId: 'Syndra', games: 2, wins: 1, losses: 1 }
	],
	recentGames: [
		{
			id: '01900000-0000-7000-8000-000000000701',
			playedAt: '2026-07-15T10:00:00Z',
			result: 'WIN' as const,
			lane: 'MID' as const,
			championId: 'Ahri'
		}
	],
	riot: {
		games: 10,
		wins: 6,
		championIds: ['Ahri', 'Orianna', 'Syndra'],
		refreshedAt: '2026-07-15T10:00:00Z'
	}
};

const snapshot: AuctionTournamentDraftSnapshot = {
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
			id: teamOneId,
			teamIndex: 1,
			teamName: '1팀',
			initialPoints: 1000,
			currentPoints: 1000,
			pointRevision: 0,
			members: [
				{
					teamMemberId,
					userId: '1525847677542207501',
					displayName: '팀장 1',
					declaredLane: 'TOP',
					mmrAtAcquisition: 1500,
					snapshotOrigin: 'CAPTURED',
					acquiredVia: 'CAPTAIN',
					rosterSlot: 1
				}
			]
		},
		{
			id: teamTwoId,
			teamIndex: 2,
			teamName: '2팀',
			initialPoints: 1000,
			currentPoints: 1000,
			pointRevision: 0,
			members: []
		}
	],
	candidates: [
		activeCandidate,
		{
			userId: queuedUserId,
			displayName: 'Nyan',
			declaredLane: 'JUNGLE',
			mmrAtStart: 1580,
			snapshotOrigin: 'CAPTURED',
			queuePosition: 1,
			state: 'QUEUED',
			drawCount: 0
		}
	],
	draft: {
		currentTarget: activeCandidate,
		currentTargetProfile: activeCandidateProfile,
		unsold: [],
		recentAction: null
	},
	permissions: {
		canManageDraft: true,
		canManageBracket: false,
		canRevertLast: false,
		canCancel: true
	}
};

function envelope(data: AuctionTournamentDraftSnapshot = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction tournament draft API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the authoritative tournament snapshot through its real detail route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope();
		});
		installBrowser(fetcher);

		await expect(fetchAuctionTournamentDraft(guildId, tournamentId)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments/${tournamentId}`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('sends one semantic draft command with the server revision, CSRF, and idempotency key', async () => {
		const nextSnapshot: AuctionTournamentDraftSnapshot = {
			...snapshot,
			tournament: { ...snapshot.tournament, revision: 8 },
			teams: [
				{
					...snapshot.teams[0]!,
					currentPoints: 850,
					pointRevision: 1,
					members: [
						...snapshot.teams[0]!.members,
						{
							teamMemberId: '01900000-0000-7000-8000-000000000622',
							userId: activeUserId,
							displayName: 'Longcat',
							declaredLane: 'MID',
							mmrAtAcquisition: 1623,
							snapshotOrigin: 'CAPTURED',
							acquiredVia: 'BID',
							rosterSlot: 2
						}
					]
				},
				snapshot.teams[1]!
			],
			candidates: snapshot.candidates.map((candidate) =>
				candidate.userId === activeUserId ? { ...candidate, state: 'ASSIGNED' as const } : candidate
			),
			draft: {
				currentTarget: null,
				currentTargetProfile: null,
				unsold: [],
				recentAction: { kind: 'AWARD_BID' }
			}
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(nextSnapshot);
		});
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			executeAuctionTournamentDraftCommand(
				guildId,
				tournamentId,
				{
					kind: 'AWARD_BID',
					expectedRevision: 7,
					memberId: activeUserId,
					teamId: teamOneId,
					points: 150
				},
				idempotencyKey
			)
		).resolves.toEqual(nextSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-tournaments/${tournamentId}/commands`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			kind: 'AWARD_BID',
			expectedRevision: 7,
			memberId: activeUserId,
			teamId: teamOneId,
			points: 150
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('preserves a conflict reason and request ID so the page can recover the authoritative snapshot', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_tournament_revision_conflict',
						message: '다른 운영자가 먼저 입찰을 반영했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await executeAuctionTournamentDraftCommand(
			guildId,
			tournamentId,
			{ kind: 'DRAW_NEXT', expectedRevision: 7 },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionTournamentDraftRevisionConflict(error)).toBe(true);
		expect(auctionTournamentDraftErrorText(error)).toBe(
			`다른 운영자가 먼저 입찰을 반영했습니다. (요청 ID: ${requestId})`
		);
	});
});
