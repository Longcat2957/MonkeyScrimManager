import { describe, expect, it } from 'vitest';
import { auctionTournamentResultSnapshotSchema } from './auctionTournamentResult';

const uuid = (index: number) => `01900000-0000-7000-8000-${String(index).padStart(12, '0')}`;
const lanes = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;

function finishedSnapshot(bestOf: 3 | 5, tied: boolean) {
	const teams = [1, 2].map((index) => ({
		id: uuid(index),
		teamIndex: index,
		teamName: `${index}팀`,
		initialPoints: 1000,
		currentPoints: 1000,
		spentPoints: 0,
		members: lanes.map((lane, slot) => ({
			teamMemberId: uuid(index * 10 + slot),
			userId: `900000000000000${index}${slot}`,
			displayName: `${index}-${lane}`,
			declaredLane: lane,
			acquiredVia: 'MANUAL',
			acquisitionPoints: 0
		}))
	}));
	const lineups = teams.flatMap((team) =>
		team.members.map((member) => ({
			teamId: team.id,
			teamMemberId: member.teamMemberId,
			userId: member.userId,
			lane: member.declaredLane
		}))
	);
	const games = (tied ? ['BLUE', 'RED'] : ['BLUE']).map((winner, index) => ({
		id: uuid(100 + index),
		setNumber: index + 1,
		sidePreset: 'DEFAULT',
		winner,
		picks: Object.fromEntries(lineups.map((slot) => [slot.userId, null])),
		bans: { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] },
		patchVersion: null
	}));
	return {
		tournament: {
			id: uuid(500),
			displayNumber: 1,
			title: '마무리 내전',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: bestOf },
			status: 'COMPLETED',
			revision: 7,
			endedEarly: true,
			championTeamId: tied ? null : teams[0]!.id,
			href: '/g/90000000000000000/auction'
		},
		teams,
		matches: [
			{
				id: uuid(600),
				matchKey: 'FINAL',
				bestOf,
				hardFearless: false,
				status: 'COMPLETED',
				endedEarly: true,
				leftTeamId: teams[0]!.id,
				rightTeamId: teams[1]!.id,
				leftScore: 1,
				rightScore: tied ? 1 : 0,
				winnerTeamId: tied ? null : teams[0]!.id,
				lineupsLocked: true,
				lineups,
				games
			}
		],
		players: [],
		permissions: { canCancel: false, canRevertResults: false, canRecoverBracket: false }
	};
}

describe('auction tournament early-finish result contract', () => {
	it.each([3, 5] as const)('keeps confirmed games for winnerless BO%s completion', (bestOf) => {
		const snapshot = finishedSnapshot(bestOf, true);
		const result = auctionTournamentResultSnapshotSchema.parse(snapshot);
		expect(result.tournament.championTeamId).toBeNull();
		expect(result.matches[0]!.games).toHaveLength(2);
		expect(result.matches[0]!.leftScore).toBe(1);
		expect(result.matches[0]!.rightScore).toBe(1);
	});

	it('accepts a leading team before the normal BO5 win threshold', () => {
		const snapshot = finishedSnapshot(5, false);
		expect(auctionTournamentResultSnapshotSchema.parse(snapshot).tournament.championTeamId).toBe(
			uuid(1)
		);
	});

	it('accepts an unfinished 20-person final without teams or games', () => {
		const snapshot = finishedSnapshot(3, true);
		const result = auctionTournamentResultSnapshotSchema.parse({
			...snapshot,
			tournament: {
				...snapshot.tournament,
				format: 'TWENTY_PERSON',
				rules: { ...snapshot.tournament.rules, semifinalBestOf: 3 }
			},
			matches: [
				{
					...snapshot.matches[0],
					leftTeamId: null,
					rightTeamId: null,
					leftScore: 0,
					rightScore: 0,
					winnerTeamId: null,
					lineupsLocked: false,
					lineups: [],
					games: []
				}
			]
		});
		expect(result.matches[0]!.endedEarly).toBe(true);
		expect(result.matches[0]!.winnerTeamId).toBeNull();
	});

	it('accepts finishing before any bracket exists', () => {
		const snapshot = finishedSnapshot(3, true);
		expect(
			auctionTournamentResultSnapshotSchema.safeParse({ ...snapshot, matches: [] }).success
		).toBe(true);
	});

	it('still rejects missing winners on normal completion and broken recorded lineups', () => {
		const snapshot = finishedSnapshot(3, true);
		expect(
			auctionTournamentResultSnapshotSchema.safeParse({
				...snapshot,
				matches: [{ ...snapshot.matches[0], endedEarly: false }]
			}).success
		).toBe(false);
		expect(
			auctionTournamentResultSnapshotSchema.safeParse({
				...snapshot,
				matches: [{ ...snapshot.matches[0], lineups: [] }]
			}).success
		).toBe(false);
	});
});
