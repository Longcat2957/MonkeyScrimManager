import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./RiotRecordsView.svelte', import.meta.url), 'utf8');

describe('RiotRecordsView request states', () => {
	it('renders safe API errors as alerts for refresh and pagination failures', () => {
		expect(source).toContain('refreshError = riotRecordsErrorText(error)');
		expect(source).toContain('loadMoreError = riotRecordsErrorText(error)');
		expect(source).toContain('class="refresh-error" role="alert"');
		expect(source).toContain('class="load-more-error" role="alert"');
	});

	it('keeps the empty recent-match state distinct from pagination failures', () => {
		expect(source).toContain('선택한 종류의 경기가 없습니다.');
		expect(source).toContain('최근 경기 더 보기');
	});
});
