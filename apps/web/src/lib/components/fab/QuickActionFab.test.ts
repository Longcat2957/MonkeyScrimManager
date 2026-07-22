import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./QuickActionFab.svelte', import.meta.url), 'utf8');

describe('QuickActionFab loading boundary', () => {
	it('loads optional tools only after the user selects them', () => {
		expect(source).not.toContain("import MiniGameModal from './MiniGameModal.svelte'");
		expect(source).not.toContain("import TimerModal from './TimerModal.svelte'");
		expect(source).toContain("await import('./MiniGameModal.svelte')");
		expect(source).toContain("await import('./TimerModal.svelte')");
	});

	it('shows a concrete loading or chunk-load failure reason', () => {
		expect(source).toContain('화면을 불러오는 중입니다.');
		expect(source).toContain('미니게임 화면을 불러오지 못했습니다.');
		expect(source).toContain('타이머 화면을 불러오지 못했습니다.');
		expect(source).toContain('role="alert"');
	});
});
