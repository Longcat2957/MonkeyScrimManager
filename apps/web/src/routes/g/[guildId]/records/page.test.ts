import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { GuildRecordsSearchSnapshot } from '$lib/contracts/workflow';
import type { ChampionCatalog } from '$lib/datadragon';
import RecordsPage from './+page.svelte';

const guildId = '1525851181157580861';
const closedSeasonId = '01900000-0000-7000-8000-000000000010';
const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

const championCatalog: ChampionCatalog = {
	version: '16.13.1',
	locale: 'ko_KR',
	champions: [],
	byId: {},
	byKey: {},
	byName: {},
	byIdSlug: {}
};

const snapshot: GuildRecordsSearchSnapshot = {
	member: { displayName: 'Longcat', avatarUrl: null, mainLane: 'MID' },
	championPreferences: {
		TOP: [],
		JUNGLE: [],
		MID: ['FutureChampion'],
		BOTTOM: [],
		SUPPORT: []
	},
	riotAccounts: [],
	seasons: [
		{
			id: closedSeasonId,
			name: '시즌 1',
			status: 'CLOSED',
			startedAt: '2026-07-01T00:00:00Z',
			endedAt: '2026-07-10T00:00:00Z'
		}
	],
	selectedSeasonId: closedSeasonId,
	summary: { games: 0, wins: 0, losses: 0 },
	laneMmr: [
		{ lane: 'TOP', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'JUNGLE', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'MID', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'BOTTOM', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'SUPPORT', mmr: 1500, games: 0, wins: 0 }
	],
	championRecords: [],
	records: [],
	nextCursor: null
};

function renderPage() {
	return render(RecordsPage, {
		props: {
			data: {
				guildId,
				query: 'Longcat',
				seasonId: closedSeasonId,
				snapshot,
				loadError: null,
				championCatalog,
				guild: {
					id: guildId,
					displayName: 'MonkeyBot Test',
					iconUrl: null,
					memberCount: 1,
					currentUserRole: 'MEMBER',
					isExternallyPublic: false,
					currentSeason: null
				}
			}
		} as never
	});
}

describe('/g/[guildId]/records page', () => {
	it('does not treat the always-present navigation object as an active navigation', () => {
		const { body } = renderPage();

		expect(body).toContain('class="custom-records-shell ');
		expect(body).not.toContain('route-loading');
		expect(body).not.toContain('<select disabled');
		expect(source).not.toContain('Boolean(navigating)');
		expect(source).toContain('navigating.to !== null');
	});

	it('keeps the no-active-season choice available while viewing a closed season', () => {
		const { body } = renderPage();

		expect(body).toContain('활성 시즌 없음');
		expect(body).toContain('시즌 1');
	});

	it('keys state and pending pagination by guild and renders unknown champions without broken images', () => {
		const { body } = renderPage();

		expect(source).toMatch(/function incomingSnapshotKey[\s\S]*data\.guildId/);
		expect(source).toContain('const sourceGuildId = data.guildId');
		expect(source).toContain('data.guildId !== sourceGuildId');
		expect(body).toContain('FutureChampion');
		expect(body).toContain('FutureChampion (이미지 없음)');
		expect(body).not.toContain('/img/champion/FutureChampion.png');
	});
});
