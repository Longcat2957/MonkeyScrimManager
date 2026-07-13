import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import type { GuildAuditEntry } from '$lib/contracts/workflow';
import GuildAuditLogPanel from './GuildAuditLogPanel.svelte';

const source = readFileSync(new URL('./GuildAuditLogPanel.svelte', import.meta.url), 'utf8');
const currentUserId = '1525847677542207548';
const entry: GuildAuditEntry = {
	id: '01900000-0000-7000-8000-000000000010',
	principalType: 'USER',
	principalId: currentUserId,
	principalDisplayName: 'Longcat',
	action: 'MEMBER_MMR_ADJUSTED',
	targetType: 'GUILD_MEMBER_MMR',
	targetId: '1525900000000000001',
	targetDisplayName: '마루',
	checkpointId: null,
	requestId: '01900000-0000-7000-8000-000000000001',
	beforeSummary: { lane: 'TOP', mmr: 1578 },
	afterSummary: { lane: 'TOP', mmr: 1596 },
	result: 'APPLIED',
	note: '누락 경기 반영',
	createdAt: '2026-07-13T05:00:00Z'
};

function renderPanel(entries: GuildAuditEntry[] = [entry], nextCursor: string | null = 'next') {
	return render(GuildAuditLogPanel, {
		props: {
			entries,
			nextCursor,
			currentUserId,
			onLoadMore: vi.fn()
		}
	});
}

describe('GuildAuditLogPanel', () => {
	it('renders authoritative entries with Korean labels and load-more affordance', () => {
		const unknown: GuildAuditEntry = {
			...entry,
			id: '01900000-0000-7000-8000-000000000011',
			action: 'FUTURE_ACTION',
			principalType: 'SYSTEM',
			principalId: 'system',
			principalDisplayName: null
		};
		const { body } = renderPanel([entry, unknown]);

		expect(body).toContain('MMR 수동 조정');
		expect(body).toContain('Longcat (나)');
		expect(body).toContain('마루 · 멤버 MMR');
		expect(body).toContain('FUTURE_ACTION');
		expect(body).toContain('기타');
		expect(body).toContain('더 보기');
		expect(body).toContain('(KST)');
	});

	it('renders a distinct empty state without fixture content', () => {
		const { body } = renderPanel([], null);

		expect(body).toContain('아직 기록된 감사 로그가 없습니다.');
		expect(body).not.toContain('MMR 수동 조정');
		expect(source).not.toContain('mockGuildAuditEntries');
	});

	it('keeps category and expanded result pills aligned with their own columns', () => {
		expect(source).toContain('onclick={() => toggleEntry(entry.raw.id)}');
		expect(source).toMatch(
			/class="grid gap-3[^"]*"[\s\S]*?<div class="justify-self-start">\s*<StatusPill\s+label=\{entry\.raw\.result/
		);
		expect(source).toContain('gap-x-3');
		expect(source).toContain('변경 전');
		expect(source).toContain('변경 후');
	});
});
