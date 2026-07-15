import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

describe('auction tournament bracket page', () => {
	it('exposes the safe bracket-unlock recovery only when the server grants it', () => {
		expect(source).toContain('snapshot.permissions.canRecoverBracket');
		expect(source).toContain("kind: 'UNLOCK_BRACKET'");
		expect(source).toContain('대진 확정 해제');
		expect(source).toContain('입력이나 결과가 하나라도 있으면');
	});

	it('returns an unlocked bracket to bidding only when the server grants draft recovery', () => {
		expect(source).toContain('snapshot.permissions.canRevertDraft');
		expect(source).toContain("kind: 'REVERT_LAST'");
		expect(source).toContain('입찰 단계로 되돌리기');
		expect(source).toContain('auction-tournament-revert-last-draft');
	});
});
