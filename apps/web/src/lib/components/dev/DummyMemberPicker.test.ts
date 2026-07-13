import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import { dummyUserFixture } from '$lib/contracts/dummyUsers.testFixtures';
import DummyMemberPicker from './DummyMemberPicker.svelte';

describe('DummyMemberPicker', () => {
	it('renders stable identity, lane, guild and selection state for feature reuse', () => {
		const { body } = render(DummyMemberPicker, {
			props: {
				users: [dummyUserFixture],
				selectedIds: [dummyUserFixture.userId],
				onSelectionChange: vi.fn()
			}
		});

		expect(body).toContain('테스트 미드 01');
		expect(body).toContain('mid-player-01');
		expect(body).toContain('MID');
		expect(body).toContain('1개 길드');
		expect(body).toContain('checked');
	});

	it('excludes users without an active membership in the required guild', () => {
		const { body } = render(DummyMemberPicker, {
			props: {
				users: [dummyUserFixture],
				selectedIds: [],
				requiredGuildId: '999999999999999999',
				onSelectionChange: vi.fn()
			}
		});

		expect(body).not.toContain('테스트 미드 01');
		expect(body).toContain('선택 가능한 사용자가 없습니다.');
	});
});
