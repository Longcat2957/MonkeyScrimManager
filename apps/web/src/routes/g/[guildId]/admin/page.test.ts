import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

describe('guild admin audit pagination', () => {
	it('deduplicates entries and refuses a repeated cursor', () => {
		expect(source).toContain('const knownIds = new Set(existingEntries.map((entry) => entry.id))');
		expect(source).toContain('snapshot.entries.filter((entry) => !knownIds.has(entry.id))');
		expect(source).toContain('snapshot.nextCursor === cursor');
		expect(source).toContain("consumedAuditCursors.has(snapshot.nextCursor ?? '')");
		expect(source).toContain('consumedAuditCursors.delete(cursor)');
	});

	it('passes API pagination state and the current user to the panel', () => {
		expect(source).toContain('fetchGuildAuditLogs(data.guildId, { limit: 50, cursor })');
		expect(source).toContain('entries={auditEntries}');
		expect(source).toContain('nextCursor={auditNextCursor}');
		expect(source).toContain('currentUserId={data.user.discordId}');
		expect(source).toContain('onLoadMore={handleAuditLoadMore}');
	});
});
