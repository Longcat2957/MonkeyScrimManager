import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const pageLoaders = [
	'./+page.server.ts',
	'./leaderboard/+page.server.ts',
	'./records/+page.server.ts',
	'./recruitments/[id]/+page.server.ts',
	'./series/+page.server.ts',
	'./auction/+page.server.ts',
	'./auction/matches/[matchId]/+page.server.ts',
	'./auction/recruitments/[id]/+page.server.ts',
	'./auction/recruitments/[id]/draft/+page.server.ts',
	'./auction/tournaments/[id]/bracket/+page.server.ts',
	'./auction/tournaments/[id]/draft/+page.server.ts',
	'./auction/tournaments/[id]/lineup/+page.server.ts',
	'./auction/tournaments/[id]/result/+page.server.ts'
];

describe('guild page server-load concurrency', () => {
	it('does not serialize page API work behind an unused parent result', () => {
		for (const loader of pageLoaders) {
			const source = readFileSync(new URL(loader, import.meta.url), 'utf8');
			expect(source, loader).not.toContain('await parent()');
			expect(source, loader).not.toMatch(/\{[^}]*\bparent\b[^}]*\}/);
		}
	});

	it('keeps the operator authorization layout dependent on guild context', () => {
		const adminLayout = readFileSync(new URL('./admin/+layout.server.ts', import.meta.url), 'utf8');
		expect(adminLayout).toContain('const { guild } = await parent()');
		expect(adminLayout).toContain("guild.currentUserRole !== 'OPERATOR'");
	});
});
