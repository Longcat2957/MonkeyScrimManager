import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

describe('auction tournament draft page', () => {
	it('allows force reset through draft recovery permission', () => {
		expect(source).toContain("command.kind === 'RESET_TO_INITIAL'");
		expect(source).toContain(
			'snapshot.permissions.canManageDraft || snapshot.permissions.canRevertLast'
		);
	});
});
