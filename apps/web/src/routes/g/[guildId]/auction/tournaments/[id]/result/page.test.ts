import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

describe('auction tournament result page', () => {
	it('uses the dedicated tournament bracket result board', () => {
		expect(source).toContain(
			"import AuctionTournamentBracketResultBoard from '$lib/components/workflow/AuctionTournamentBracketResultBoard.svelte'"
		);
		expect(source).toContain('<AuctionTournamentBracketResultBoard');
		expect(source).toContain('onOpenMatch={openMatch}');
		expect(source).not.toContain('AuctionTournamentBracketMatchSetupBoard');
		expect(source).not.toContain('AuctionTournamentOverviewBoard');
	});

	it('keeps safe bracket recovery available from the in-game result route', () => {
		expect(source).toContain('data.snapshot.permissions.canRecoverBracket');
		expect(source).toContain("kind: 'UNLOCK_BRACKET'");
		expect(source).toContain('대진 확정 해제');
	});
});
