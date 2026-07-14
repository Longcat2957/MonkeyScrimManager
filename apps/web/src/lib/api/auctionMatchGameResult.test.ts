import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	validateAuctionMatchGameDraft,
	type AuctionMatchGameResultSnapshot
} from '$lib/contracts/auctionMatchGameResult';
import {
	auctionMatchGameResultErrorText,
	fetchAuctionMatchGameResult,
	isAuctionMatchGameResultRevisionConflict,
	revertLastAuctionMatchGameResult,
	submitAuctionMatchGameResult
} from './auctionMatchGameResult';

const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const matchId = '01900000-0000-7000-8000-000000000701';
const leftTeamId = '01900000-0000-7000-8000-000000000711';
const rightTeamId = '01900000-0000-7000-8000-000000000712';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const lanes = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;

function team(teamId: string, number: 1 | 2) {
	return {
		id: teamId,
		teamName: `${number}팀`,
		members: lanes.map((declaredLane, index) => ({
			teamMemberId: `01900000-0000-7000-8000-000000000${number === 1 ? 72 : 73}${index + 1}`,
			userId: `15258476775422075${String((number - 1) * 5 + index + 1).padStart(2, '0')}`,
			displayName: `${number}팀 선수 ${index + 1}`,
			declaredLane,
			mmrAtAcquisition: 1600 + index,
			snapshotOrigin: 'CAPTURED' as const,
			acquiredVia: 'BID' as const,
			rosterSlot: index + 1
		}))
	};
}

const teams = [team(leftTeamId, 1), team(rightTeamId, 2)];
const lineups = teams.flatMap((currentTeam) =>
	currentTeam.members.map((member, index) => ({
		teamId: currentTeam.id,
		lane: lanes[index]!,
		teamMemberId: member.teamMemberId,
		userId: member.userId
	}))
);
const emptyPicks = Object.fromEntries(lineups.map((lineup) => [lineup.userId, null]));
const emptyBans = { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] };

const snapshot: AuctionMatchGameResultSnapshot = {
	match: {
		id: matchId,
		tournamentId,
		matchKey: 'FINAL',
		label: '결승',
		bestOf: 3,
		hardFearless: true,
		status: 'READY',
		revision: 7,
		leftTeamId,
		rightTeamId,
		leftScore: 0,
		rightScore: 0,
		winnerTeamId: null,
		lineupsLocked: true
	},
	teams,
	lineups,
	games: [],
	permissions: {
		canManageLineup: true,
		canEnterResults: true,
		canUnlockLineup: false,
		canRevertLastGame: false
	}
};

function envelope(data: AuctionMatchGameResultSnapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction match game result API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the complete match snapshot from the canonical match route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(snapshot);
		});
		installBrowser(fetcher);

		await expect(fetchAuctionMatchGameResult(guildId, matchId)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
	});

	it('submits and reverts a result with the match revision, CSRF, and idempotency key', async () => {
		const submitted: AuctionMatchGameResultSnapshot = {
			...snapshot,
			match: { ...snapshot.match, revision: 8, status: 'IN_PROGRESS', leftScore: 1 },
			games: [
				{
					id: '01900000-0000-7000-8000-000000000801',
					setNumber: 1,
					sidePreset: 'DEFAULT',
					winner: 'BLUE',
					picks: { ...emptyPicks, [lineups[0]!.userId]: 'Aatrox' },
					bans: emptyBans,
					patchVersion: '16.13.1'
				}
			],
			permissions: { ...snapshot.permissions, canRevertLastGame: true }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			const [, init] = args;
			return envelope(init?.method === 'DELETE' ? snapshot : submitted);
		});
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			submitAuctionMatchGameResult(
				guildId,
				matchId,
				{
					expectedRevision: 7,
					setNumber: 1,
					sidePreset: 'DEFAULT',
					winner: 'BLUE',
					picks: submitted.games[0]!.picks,
					bans: emptyBans,
					patchVersion: '16.13.1'
				},
				idempotencyKey
			)
		).resolves.toEqual(submitted);
		await expect(
			revertLastAuctionMatchGameResult(guildId, matchId, { expectedRevision: 8 }, idempotencyKey)
		).resolves.toEqual(snapshot);

		const [submitPath, submitInit] = fetcher.mock.calls[0]!;
		expect(submitPath).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}/games`);
		expect(submitInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(submitInit?.body))).toMatchObject({
			expectedRevision: 7,
			setNumber: 1,
			winner: 'BLUE'
		});

		const [revertPath, revertInit] = fetcher.mock.calls[1]!;
		expect(revertPath).toBe(`/api/guilds/${guildId}/auction-matches/${matchId}/games/last`);
		expect(revertInit).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(revertInit?.body))).toEqual({ expectedRevision: 8 });
		for (const init of [submitInit, revertInit]) {
			const headers = new Headers(init?.headers);
			expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
			expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		}
	});

	it('surfaces revision conflict details and limits hard fearless reuse to prior and current picks', async () => {
		const priorGame = {
			id: '01900000-0000-7000-8000-000000000801',
			setNumber: 1,
			sidePreset: 'DEFAULT' as const,
			winner: 'BLUE' as const,
			picks: { ...emptyPicks, [lineups[0]!.userId]: 'Aatrox' },
			bans: emptyBans,
			patchVersion: '16.13.1'
		};
		const completedFirstSet: AuctionMatchGameResultSnapshot = {
			...snapshot,
			match: { ...snapshot.match, revision: 8, status: 'IN_PROGRESS', leftScore: 1 },
			games: [priorGame]
		};
		expect(
			validateAuctionMatchGameDraft(completedFirstSet, {
				picks: { ...emptyPicks, [lineups[1]!.userId]: 'Aatrox' },
				bans: emptyBans
			})
		).toEqual({ duplicateChampionIds: [], hardFearlessChampionIds: ['Aatrox'] });
		expect(
			validateAuctionMatchGameDraft(completedFirstSet, {
				picks: emptyPicks,
				bans: { ...emptyBans, BLUE: ['Aatrox', null, null, null, null] }
			})
		).toEqual({ duplicateChampionIds: [], hardFearlessChampionIds: [] });
		const priorBanOnly: AuctionMatchGameResultSnapshot = {
			...completedFirstSet,
			games: [
				{
					...priorGame,
					picks: emptyPicks,
					bans: { ...emptyBans, BLUE: ['Yone', null, null, null, null] }
				}
			]
		};
		expect(
			validateAuctionMatchGameDraft(priorBanOnly, {
				picks: { ...emptyPicks, [lineups[1]!.userId]: 'Yone' },
				bans: emptyBans
			})
		).toEqual({ duplicateChampionIds: [], hardFearlessChampionIds: [] });

		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_match_revision_conflict',
						message: '다른 운영자가 먼저 게임 결과를 기록했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await revertLastAuctionMatchGameResult(
			guildId,
			matchId,
			{ expectedRevision: 7 },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionMatchGameResultRevisionConflict(error)).toBe(true);
		expect(auctionMatchGameResultErrorText(error)).toBe(
			`다른 운영자가 먼저 게임 결과를 기록했습니다. (요청 ID: ${requestId})`
		);
	});

	it('does not classify a non-revision conflict as a revision conflict', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_match_lineup_locked',
						message: '라인업을 다시 확인해 주세요.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await revertLastAuctionMatchGameResult(
			guildId,
			matchId,
			{ expectedRevision: 7 },
			idempotencyKey
		).catch((cause) => cause);

		expect(isAuctionMatchGameResultRevisionConflict(error)).toBe(false);
		expect(auctionMatchGameResultErrorText(error)).toBe(
			`라인업을 다시 확인해 주세요. (요청 ID: ${requestId})`
		);
	});
});
