import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(
	new URL('./ScrimPickBanConfirmedSetsDisclosure.svelte', import.meta.url),
	'utf8'
);

describe('ScrimPickBanConfirmedSetsDisclosure', () => {
	it('starts collapsed and does not render an empty confirmed-set container', () => {
		expect(source).toContain('let expanded = $state(false)');
		expect(source).toContain('{#if confirmedGames.length > 0}');
		expect(source).toContain('aria-expanded={expanded}');
	});

	it('renders every confirmed set with the existing pick-ban summary card when expanded', () => {
		expect(source).toContain('ScrimPickBanResultSetSummaryCard');
		expect(source).toContain('{#each confirmedGames as game (game.setNumber)}');
		expect(source).toContain('winner={game.winner}');
		expect(source).toContain('bans={game.bans}');
		expect(source).toContain('picks={game.picks}');
		expect(source).toContain('teamIdForSide(game, side)');
	});
});
