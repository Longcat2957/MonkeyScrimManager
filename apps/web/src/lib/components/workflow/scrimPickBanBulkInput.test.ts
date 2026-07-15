import { describe, expect, it } from 'vitest';
import {
	scrimPickBanResultInputChampions,
	mockScrimPickBanResultInputTeams
} from '$lib/fixtures/scrimPickBanResultInputFixtures';
import {
	buildScrimPickBanBulkInputPlan,
	emptyScrimPickBanBulkInputValues
} from './scrimPickBanBulkInput';

const emptyPicks = Object.fromEntries(
	Object.values(mockScrimPickBanResultInputTeams)
		.flatMap((team) => team.players)
		.map((player) => [player.id, null])
);
const emptyBans = { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] };

function buildPlan(options: Partial<Parameters<typeof buildScrimPickBanBulkInputPlan>[0]> = {}) {
	return buildScrimPickBanBulkInputPlan({
		values: {
			...emptyScrimPickBanBulkInputValues(),
			BLUE_PICKS: '나르, 비에고, 아리, 이즈리얼, 쓰레쉬',
			...options.values
		},
		teams: mockScrimPickBanResultInputTeams,
		picks: emptyPicks,
		bans: emptyBans,
		champions: scrimPickBanResultInputChampions,
		hardFearless: false,
		previousPickChampionIds: [],
		...options
	});
}

describe('scrim pick-ban bulk input plan', () => {
	it('creates semantic SET_PICK items in lane order without sequential requests', () => {
		const plan = buildPlan();

		expect(plan.canApply).toBe(true);
		expect(plan.hasValidationErrors).toBe(false);
		expect(plan.operations).toEqual([
			{ type: 'SET_PICK', playerId: 'blue-top', championId: 'Gnar' },
			{ type: 'SET_PICK', playerId: 'blue-jungle', championId: 'Viego' },
			{ type: 'SET_PICK', playerId: 'blue-mid', championId: 'Ahri' },
			{ type: 'SET_PICK', playerId: 'blue-bottom', championId: 'Ezreal' },
			{ type: 'SET_PICK', playerId: 'blue-support', championId: 'Thresh' }
		]);
	});

	it('uses the selected game-side teams when an auction match swaps sides', () => {
		const swappedGameSideTeams = {
			BLUE: { ...mockScrimPickBanResultInputTeams.RED, side: 'BLUE' as const },
			RED: { ...mockScrimPickBanResultInputTeams.BLUE, side: 'RED' as const }
		};

		const plan = buildPlan({ teams: swappedGameSideTeams });

		expect(plan.canApply).toBe(true);
		expect(plan.operations).toEqual(
			expect.arrayContaining([
				{ type: 'SET_PICK', playerId: 'red-top', championId: 'Gnar' },
				{ type: 'SET_PICK', playerId: 'red-jungle', championId: 'Viego' },
				{ type: 'SET_PICK', playerId: 'red-mid', championId: 'Ahri' },
				{ type: 'SET_PICK', playerId: 'red-bottom', championId: 'Ezreal' },
				{ type: 'SET_PICK', playerId: 'red-support', championId: 'Thresh' }
			])
		);
	});

	it('allows prior-set picks in a standard series but blocks them in hard fearless', () => {
		expect(buildPlan({ previousPickChampionIds: ['Gnar'] }).canApply).toBe(true);

		const hardFearlessPlan = buildPlan({
			hardFearless: true,
			previousPickChampionIds: ['Gnar']
		});
		expect(hardFearlessPlan.canApply).toBe(false);
		expect(hardFearlessPlan.fields[0]?.feedback).toEqual(
			expect.arrayContaining([
				expect.objectContaining({ severity: 'error', message: expect.stringContaining('Hard Fearless') })
			])
		);
	});

	it('validates the synthesized final draft across picks and bans', () => {
		const plan = buildPlan({
			values: {
				...emptyScrimPickBanBulkInputValues(),
				BLUE_PICKS: '나르',
				RED_BANS: '나르'
			}
		});

		expect(plan.canApply).toBe(false);
		expect(plan.hasValidationErrors).toBe(true);
		expect(plan.fields.flatMap((field) => field.feedback).map((feedback) => feedback.message)).toEqual(
			expect.arrayContaining([expect.stringContaining('중복')])
		);
	});
});
