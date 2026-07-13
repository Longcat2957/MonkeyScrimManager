import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { GuildAdminSeasonsSnapshot } from '$lib/contracts/workflow';
import Page from './+page.svelte';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const guildId = '1525851181157580861';
const currentSeasonId = '01900000-0000-7000-8000-000000000010';
const emptySnapshot: GuildAdminSeasonsSnapshot = {
	current: {
		id: currentSeasonId,
		name: '시즌 1',
		status: 'ACTIVE',
		startedAt: '2026-07-13T01:00:00Z',
		endedAt: null,
		createdBy: null,
		participantCount: 0,
		generalSeriesCount: 0,
		auctionTournamentCount: 0,
		auctionTenCount: 0,
		auctionTwentyCount: 0,
		confirmedGameCount: 0,
		lastGameAt: null
	},
	completed: []
};

function renderPage(seasonSnapshot: GuildAdminSeasonsSnapshot) {
	return render(Page, {
		props: {
			data: { guildId, seasonSnapshot }
		} as never
	});
}

describe('guild admin seasons page', () => {
	it('renders the authoritative current season and an explicit empty history state', () => {
		const { body } = renderPage(emptySnapshot);

		expect(body).toContain('현재 시즌');
		expect(body).toContain('시즌 1');
		expect(body).toContain('2026. 07. 13.');
		expect(body).toContain('시스템');
		expect(body).toContain('일반 시리즈');
		expect(body).toContain('경매 내전');
		expect(body).toContain('확정 게임');
		expect(body).toContain('종료된 시즌이 없습니다.');
		expect(body).toContain('aria-haspopup="dialog"');
	});

	it('expands the first completed season and formats timestamps in Seoul', () => {
		const snapshot: GuildAdminSeasonsSnapshot = {
			...emptySnapshot,
			completed: [
				{
					...emptySnapshot.current,
					id: '01900000-0000-7000-8000-000000000009',
					name: '2026 시즌 2',
					status: 'CLOSED',
					startedAt: '2026-04-01T15:00:00Z',
					endedAt: '2026-06-30T14:00:00Z',
					createdBy: 'Longcat',
					participantCount: 46,
					generalSeriesCount: 24,
					auctionTournamentCount: 16,
					auctionTenCount: 10,
					auctionTwentyCount: 6,
					confirmedGameCount: 96,
					lastGameAt: '2026-06-29T14:18:00Z'
				}
			]
		};
		const { body } = renderPage(snapshot);

		expect(body).toContain('2026 시즌 2');
		expect(body).toContain('aria-expanded="true"');
		expect(body).toContain('2026. 04. 02. - 2026. 06. 30.');
		expect(body).toContain('2026. 06. 29. 23:18');
		expect(body).toContain('10인 10 · 20인 6');
		expect(body).toContain('Longcat');
	});

	it('keeps rotation authoritative, idempotent, locked while pending, and name-only', () => {
		expect(source).toContain('startGuildAdminSeason(data.guildId');
		expect(source).toContain('mutationIdempotencyKey = globalThis.crypto.randomUUID()');
		expect(source).toContain('expectedCurrentSeasonId');
		expect(source).toContain("cause.code === 'current_season_conflict'");
		expect(source).toContain('seasonSnapshot = authoritative');
		expect(source).toContain('disabled={!canStartSeason || submitting}');
		expect(source).toContain('type="button"');
		expect(source).toContain('maxlength="100"');
		expect(source).not.toContain('type="date"');
	});
});
