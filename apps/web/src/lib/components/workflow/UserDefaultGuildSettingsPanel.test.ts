import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import UserDefaultGuildSettingsPanel from './UserDefaultGuildSettingsPanel.svelte';

const guild = {
	id: '200000000000000001',
	displayName: 'Monkey Scrim Team',
	iconUrl: null,
	memberCount: 12,
	currentUserRole: 'MEMBER' as const
};

function openingTag(body: string, marker: string): string {
	const markerIndex = body.indexOf(marker);
	expect(markerIndex).toBeGreaterThanOrEqual(0);
	const start = body.lastIndexOf('<', markerIndex);
	const end = body.indexOf('>', markerIndex);
	return body.slice(start, end + 1);
}

function isDisabled(tag: string): boolean {
	return /\sdisabled(?:[=\s>])/.test(tag);
}

describe('UserDefaultGuildSettingsPanel', () => {
	it('keeps an absent saved guild distinct from the first draft option', () => {
		const { body } = render(UserDefaultGuildSettingsPanel, {
			props: { initialGuildId: null, guilds: [guild], onSave: vi.fn() }
		});

		expect(openingTag(body, `value="${guild.id}"`)).toContain('selected');
		expect(body).toContain('대표 길드 미설정');
		expect(isDisabled(openingTag(body, 'aria-label="대표 길드 저장"'))).toBe(false);
		expect(isDisabled(openingTag(body, 'aria-label="대표 길드 변경 취소"'))).toBe(false);
	});

	it('does not mark an already saved guild as dirty', () => {
		const { body } = render(UserDefaultGuildSettingsPanel, {
			props: { initialGuildId: guild.id, guilds: [guild], onSave: vi.fn() }
		});

		expect(isDisabled(openingTag(body, 'aria-label="대표 길드 저장"'))).toBe(true);
		expect(isDisabled(openingTag(body, 'aria-label="대표 길드 변경 취소"'))).toBe(true);
	});
});
