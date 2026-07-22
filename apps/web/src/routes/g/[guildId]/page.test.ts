import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { GuildContextData } from '$lib/contracts/workflow';
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

function renderPage(guild: GuildContextData) {
	return render(Page, {
		props: {
			data: {
				guildId,
				guild,
				operations: []
			}
		} as never
	});
}

describe('/g/[guildId] dashboard', () => {
	it('shows current season context and operator launch paths when the queue is empty', () => {
		const { body } = renderPage(baseGuild);

		expect(body).toContain('2026 여름 시즌 · 48명 · 운영자');
		expect(body).toContain('현재 처리할 운영 작업이 없습니다.');
		expect(body).toContain(`href="/g/${guildId}/series"`);
		expect(body).toContain('내전 운영');
		expect(body).toContain(`href="/g/${guildId}/auction"`);
		expect(body).toContain('경매 운영');
	});

	it('uses participant-oriented empty guidance and handles a missing active season', () => {
		const { body } = renderPage({
			...baseGuild,
			currentUserRole: 'MEMBER',
			currentSeason: null
		});

		expect(body).toContain('진행 중 시즌 없음 · 48명 · 참가자');
		expect(body).toContain('현재 진행 중인 길드 작업이 없습니다.');
		expect(body).toContain(`href="/g/${guildId}/leaderboard"`);
		expect(body).toContain('리더보드 보기');
		expect(body).not.toContain('경매 운영');
	});
});
