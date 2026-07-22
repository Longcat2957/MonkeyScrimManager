import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./AppShell.svelte', import.meta.url), 'utf8');

describe('AppShell guild navigation', () => {
	it('keeps the logo in the active guild context', () => {
		expect(source).toContain("const homeHref = $derived(activeGuildBase || resolve('/'))");
		expect(source).toContain('href={homeHref}');
		expect(source).toContain("'현재 길드 대시보드로 이동'");
	});

	it('treats recruitment and series routes as the same scrim navigation area', () => {
		expect(source).toContain(
			'matches: [`${activeGuildBase}/series`, `${activeGuildBase}/recruitments`]'
		);
	});

	it('marks only exact routes and their descendants as active', () => {
		expect(source).toContain('pathname === match || pathname.startsWith(`${match}/`)');
		expect(source).toContain("aria-current={active ? 'page' : undefined}");
		expect(source).not.toContain('pathname.startsWith(match)');
	});

	it('switches guilds through the selected guild dashboard', () => {
		expect(source).toContain("goto(resolve('/g/[guildId]', { guildId }))");
	});
});

describe('AppShell search errors', () => {
	it('shows the safe API reason instead of silently dropping suggestion failures', () => {
		expect(source).toContain('searchSuggestionsError = searchSuggestionsErrorText(error)');
		expect(source).toContain('{:else if searchSuggestionsError}');
		expect(source).toContain('<div role="alert"');
	});
});
