import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const riotAccounts = readFileSync(new URL('./RiotAccountManager.svelte', import.meta.url), 'utf8');
const privacy = readFileSync(new URL('./UserPrivacySettingsPanel.svelte', import.meta.url), 'utf8');
const champions = readFileSync(
	new URL('./UserChampionPreferencesEditor.svelte', import.meta.url),
	'utf8'
);

describe('/me settings error presentation', () => {
	it('uses public API error formatters that preserve safe reasons and request IDs', () => {
		expect(riotAccounts).toContain('error = riotErrorText(');
		expect(privacy).toContain('error = privacySettingsErrorText(saveError)');
		expect(champions).toContain('error = championPreferencesErrorText(saveError)');
	});

	it('announces each rendered failure to assistive technology', () => {
		expect(riotAccounts).toContain('aria-live="assertive"');
		expect(privacy).toContain('aria-live="assertive"');
		expect(champions).toContain('aria-live="assertive"');
	});
});
