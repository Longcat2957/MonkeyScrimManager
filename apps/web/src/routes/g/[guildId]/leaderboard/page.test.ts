import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

describe('guild leaderboard page', () => {
	it('opens a double-clicked player through the exact Discord ID guild-records query', () => {
		expect(source).toContain("import { goto } from '$app/navigation'");
		expect(source).toContain("resolve('/g/[guildId]/records', { guildId: data.guildId })");
		expect(source).toContain('new globalThis.URLSearchParams({ q: playerId })');
		expect(source).toContain("query.set('seasonId', data.snapshot.season.id)");
		expect(source).toContain('onOpenPlayer={openPlayerRecords}');
	});
});
