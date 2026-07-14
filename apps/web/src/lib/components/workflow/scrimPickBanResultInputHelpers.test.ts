import { describe, expect, it } from 'vitest';
import {
	mockScrimPickBanResultInputGames,
	mockScrimPickBanResultInputTeams
} from '$lib/fixtures/scrimPickBanResultInputFixtures';
import {
	adjacentScrimPickBanResultInputCell,
	scrimPickBanResultInputCellId,
	scrimPickBanResultInputCellOrder
} from './scrimPickBanResultInputHelpers';

describe('scrim pick-ban input navigation', () => {
	const cells = scrimPickBanResultInputCellOrder(
		mockScrimPickBanResultInputTeams,
		mockScrimPickBanResultInputGames[0]!.bans
	);

	it('follows the established all-bans then all-picks slot order', () => {
		expect(cells.slice(0, 3)).toEqual([
			{ kind: 'BAN', key: 'BLUE-ban-0' },
			{ kind: 'BAN', key: 'BLUE-ban-1' },
			{ kind: 'BAN', key: 'BLUE-ban-2' }
		]);
		expect(cells[5]).toEqual({ kind: 'BAN', key: 'RED-ban-0' });
		expect(cells[10]).toEqual({ kind: 'PICK', key: 'blue-top' });
		expect(cells.at(-1)).toEqual({ kind: 'PICK', key: 'red-support' });
	});

	it('wraps Tab and Shift+Tab across the complete ban/pick sequence', () => {
		expect(adjacentScrimPickBanResultInputCell(cells, cells.at(-1)!, 1)).toEqual(cells[0]);
		expect(adjacentScrimPickBanResultInputCell(cells, cells[0]!, -1)).toEqual(cells.at(-1));
		expect(adjacentScrimPickBanResultInputCell(cells, cells[9]!, 1)).toEqual(cells[10]);
	});

	it('gives every navigable field a stable focus target', () => {
		const ids = cells.map(scrimPickBanResultInputCellId);
		expect(new Set(ids).size).toBe(cells.length);
		expect(ids[0]).toBe('scrim-pickban-ban-BLUE-ban-0');
		expect(ids.at(-1)).toBe('scrim-pickban-pick-red-support');
	});
});
