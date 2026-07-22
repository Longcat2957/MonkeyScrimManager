import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const rootLayout = readFileSync(new URL('./+layout.svelte', import.meta.url), 'utf8');
const errorPage = readFileSync(new URL('./+error.svelte', import.meta.url), 'utf8');
const appRouteShell = readFileSync(
	new URL('../lib/components/layout/AppRouteShell.svelte', import.meta.url),
	'utf8'
);
const appRouteLayouts = [
	'admin',
	'dd',
	'dev',
	'docs',
	'g',
	'me',
	'privacy',
	'records',
	'terms',
	'users'
].map((route) => readFileSync(new URL(`./${route}/+layout.svelte`, import.meta.url), 'utf8'));

describe('route shell loading boundary', () => {
	it('keeps the root layout free of authenticated shell dependencies', () => {
		expect(rootLayout).not.toContain('AppShell');
		expect(rootLayout).not.toContain('$app/state');
		expect(rootLayout).toContain('{@render children()}');
	});

	it('wraps every existing application route root with the shared AppShell boundary', () => {
		expect(appRouteShell).toContain("import AppShell from './AppShell.svelte'");
		expect(appRouteShell).toContain('<AppShell>{@render children()}</AppShell>');
		for (const layout of appRouteLayouts) {
			expect(layout).toContain(
				"import AppRouteShell from '$lib/components/layout/AppRouteShell.svelte'"
			);
		}
	});

	it('preserves the public error-page boundary without eagerly loading AppShell on success', () => {
		expect(errorPage).toContain("['/landing', '/install']");
		expect(errorPage).toContain('{#if isPublicRoute}');
		expect(errorPage).toContain('<AppShell>{@render errorContent()}</AppShell>');
	});
});
