import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { GuildContextData } from '$lib/contracts/workflow';
import type { OperationViewItem } from '$lib/workflow/presentation';
import Page from './+page.svelte';

const guildId = '1525851181157580861';
const baseGuild: GuildContextData = {
	id: guildId,
	displayName: '원숭이 내전방',
	iconUrl: null,
	memberCount: 48,
	currentUserRole: 'OPERATOR',
	isExternallyPublic: false,
	currentSeason: {
		id: '01900000-0000-7000-8000-000000000010',
		name: '2026 여름 시즌',
		startedAt: '2026-07-01T00:00:00Z'
	}
};

function renderPage(guild: GuildContextData, operations: OperationViewItem[] = []) {
	return render(Page, {
		props: {
			data: {
				guildId,
				guild,
				operations
			}
		} as never
	});
}

function expectSingleCreateLinks(body: string) {
	expect(body.match(new RegExp(`href="/g/${guildId}/series\\?create=1"`, 'g')) ?? []).toHaveLength(
		1
	);
	expect(body.match(new RegExp(`href="/g/${guildId}/auction\\?create=1"`, 'g')) ?? []).toHaveLength(
		1
	);
}

const activeOperation: OperationViewItem = {
	id: 'series:01900000-0000-7000-8000-000000000201',
	kind: 'PICK_BAN',
	title: '금요 정기 내전',
	status: '픽밴/결과 입력',
	detail: '2세트 입력 대기',
	updatedAt: '8분 전',
	href: `/g/${guildId}/series/01900000-0000-7000-8000-000000000201/pick-ban`,
	priority: 'urgent',
	actionLabel: '픽밴 진행',
	tone: 'warning'
};

describe('/g/[guildId] dashboard', () => {
	it('shows current season context and operator launch paths when the queue is empty', () => {
		const { body } = renderPage(baseGuild);

		expect(body).toContain('2026 여름 시즌 · 48명 · 운영자');
		expect(body).toContain('현재 처리할 운영 작업이 없습니다.');
		expect(body).toContain(`href="/g/${guildId}/series?create=1"`);
		expect(body).toContain('새 내전 모집');
		expect(body).toContain(`href="/g/${guildId}/auction?create=1"`);
		expect(body).toContain('새 경매 모집');
		expectSingleCreateLinks(body);
	});

	it('uses participant-oriented empty guidance and handles a missing active season', () => {
		const { body } = renderPage({
			...baseGuild,
			currentUserRole: 'MEMBER',
			currentSeason: null
		});

		expect(body).toContain('진행 중 시즌 없음 · 48명 · 참가자');
		expect(body).toContain('현재 진행 중인 길드 작업이 없습니다.');
		expect(body).toContain('길드 멤버는 새 내전이나 경매 모집을 직접 시작할 수 있습니다.');
		expect(body).toContain(`href="/g/${guildId}/series?create=1"`);
		expect(body).toContain(`href="/g/${guildId}/auction?create=1"`);
		expectSingleCreateLinks(body);
	});

	it('keeps recruitment launch paths available while active work is visible', () => {
		const { body } = renderPage(baseGuild, [activeOperation]);

		expect(body).toContain('금요 정기 내전');
		expect(body).toContain(`href="/g/${guildId}/series?create=1"`);
		expect(body).toContain('새 내전');
		expect(body).toContain(`href="/g/${guildId}/auction?create=1"`);
		expect(body).toContain('새 경매');
		expect(body).not.toContain('현재 처리할 운영 작업이 없습니다.');
		expectSingleCreateLinks(body);
	});
});
