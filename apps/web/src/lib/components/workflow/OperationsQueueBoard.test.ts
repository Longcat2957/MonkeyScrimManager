import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./OperationsQueueBoard.svelte', import.meta.url), 'utf8');

describe('operations queue board', () => {
	it('renders presentation-provided actions and tones without reinterpreting workflow kinds', () => {
		expect(source).toContain('<StatusPill tone={item.tone} label={item.status} />');
		expect(source).toContain('{item.actionLabel}');
		expect(source).not.toContain('function statusTone');
		expect(source).not.toContain('function actionLabel');
	});

	it('only shows an explicitly labelled urgent count when urgent work exists', () => {
		expect(source).toContain('{#if urgentCount > 0}');
		expect(source).toContain('aria-label={`긴급 작업 ${urgentCount}건`}');
		expect(source).toContain('긴급 {urgentCount}');
	});
});
