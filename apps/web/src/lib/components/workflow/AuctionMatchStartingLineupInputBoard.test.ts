import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { AuctionMatchLineupSnapshot } from '$lib/contracts/auctionMatchLineup';
import AuctionMatchStartingLineupInputBoard from './AuctionMatchStartingLineupInputBoard.svelte';

const source = readFileSync(
	new URL('./AuctionMatchStartingLineupInputBoard.svelte', import.meta.url),
	'utf8'
);
const tournamentId = '01900000-0000-7000-8000-000000000601';
const matchId = '01900000-0000-7000-8000-000000000701';
const leftTeamId = '01900000-0000-7000-8000-000000000711';
const rightTeamId = '01900000-0000-7000-8000-000000000712';
const leftMemberId = '01900000-0000-7000-8000-000000000721';
const rightMemberId = '01900000-0000-7000-8000-000000000722';

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
			members: [
				{
					teamMemberId: leftMemberId,
					userId: '1525847677542207501',
					displayName: 'Longcat',
					declaredLane: 'MID',
					mmrAtAcquisition: 1623.7499999999998,
					snapshotOrigin: 'CAPTURED',
					acquiredVia: 'CAPTAIN',
					rosterSlot: 1
				}
			]
		},
		{
			id: rightTeamId,
			teamName: '2팀',
			members: [
				{
					teamMemberId: rightMemberId,
					userId: '1525847677542207502',
					displayName: 'Nyan',
					declaredLane: 'TOP',
					mmrAtAcquisition: 1588,
					snapshotOrigin: 'CAPTURED',
					acquiredVia: 'CAPTAIN',
					rosterSlot: 1
				}
			]
		}
	],
	lineups: [
		{
			teamId: leftTeamId,
			lane: 'MID',
			teamMemberId: leftMemberId,
			userId: '1525847677542207501'
		}
	],
	permissions: {
		canManageLineup: true,
		canEnterResults: false,
		canUnlockLineup: false,
		canRevertLastGame: false
	}
};

describe('AuctionMatchStartingLineupInputBoard', () => {
	it('renders the server snapshot with selectable controlled lineup slots', () => {
		const { body } = render(AuctionMatchStartingLineupInputBoard, {
			props: {
				snapshot,
				onAssign: async () => undefined,
				onLock: async () => undefined
			}
		});

		expect(body).toContain('결승 라인업');
		expect(body).toContain('BO3 · 하드 피어리스');
		expect(body).toContain('Longcat');
		expect(body).toContain('Nyan');
		expect(body).toContain('MMR 1,624');
		expect(body).not.toContain('1623.7499999999998');
		expect(body).toContain('선수 선택');
	});

	it('uses real controlled callbacks and mobile-safe select controls without fixture persistence', () => {
		expect(source).toContain('snapshot: AuctionMatchLineupSnapshot');
		expect(source).toContain('onAssign?: (command: AuctionMatchLineupAssignCommand)');
		expect(source).toContain('onLock?: (command: AuctionMatchLineupLockCommand)');
		expect(source).toContain('onUnlock?: (command: AuctionMatchLineupUnlockCommand)');
		expect(source).toContain('라인업 다시 편성');
		expect(source).toContain('isAssignedElsewhere');
		expect(source).toContain('<select');
		expect(source).not.toContain('$lib/fixtures');
		expect(source).not.toContain('draggable=');
		expect(source).not.toContain('let lineup = $state');
	});
});
