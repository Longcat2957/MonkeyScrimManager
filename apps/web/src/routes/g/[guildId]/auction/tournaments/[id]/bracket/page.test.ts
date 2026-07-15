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
});
