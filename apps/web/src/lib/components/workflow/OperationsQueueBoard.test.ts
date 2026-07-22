import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { OperationViewItem } from '$lib/workflow/presentation';
import OperationsQueueBoard from './OperationsQueueBoard.svelte';

const source = readFileSync(new URL('./OperationsQueueBoard.svelte', import.meta.url), 'utf8');

function operation(
	id: string,
	kind: OperationViewItem['kind'],
	status: string,
	priority: OperationViewItem['priority'],
	actionLabel: string
): OperationViewItem {
	return {
		id,
		kind,
		title: `${id} 작업`,
		status,
		detail: '다음 입력 대기',
		updatedAt: '3분 전',
		href: `/g/200000000000000001/work/${id}`,
		priority,
		actionLabel,
		tone: priority === 'urgent' ? 'warning' : 'info'
	};
}

const items = [
	operation('auction', 'AUCTION', '입찰 진행', 'normal', '입찰 진행'),
	operation('entry', 'ENTRY', '편성 필요', 'urgent', '엔트리 편성'),
	operation('pick-ban', 'PICK_BAN', '픽밴/결과 입력', 'urgent', '픽밴 진행')
];

describe('operations queue board', () => {
	it('spotlights the first urgent operation once and leaves the remaining queue deduplicated', () => {
		const { body } = render(OperationsQueueBoard, {
			props: { items, currentUserRole: 'OPERATOR' }
		});

		expect(body).toContain('지금 처리할 작업');
		expect(body).toContain('entry 작업');
		expect(body.match(/href="\/g\/200000000000000001\/work\/entry"/g)).toHaveLength(1);
		expect(body).toContain('엔트리 편성');
		expect(body).toContain('aria-label="entry 작업 진행 단계"');
		expect(body).toContain('aria-current="step">편성');
		expect(body).toContain('남은 운영 작업');
		expect(body).toContain('aria-label="남은 긴급 작업 1건"');
		expect(body).toContain('auction 작업');
		expect(body).toContain('pick-ban 작업');
	});

	it('uses observation-oriented actions for guild members', () => {
		const { body } = render(OperationsQueueBoard, {
			props: { items: [items[1]], currentUserRole: 'MEMBER' }
		});

		expect(body).toContain('현재 진행 상황');
		expect(body).toContain('진행 상황 보기');
		expect(body).not.toContain('지금 처리할 작업');
		expect(body).not.toContain('엔트리 편성');
	});

	it('keeps workflow interpretation in the presentation layer', () => {
		expect(source).toContain('selectPrimaryOperation(items)');
		expect(source).toContain('operationStageView(primaryItem)');
		expect(source).toContain('operationActionLabelForRole(item, currentUserRole)');
		expect(source).not.toContain('function statusTone');
	});
});
