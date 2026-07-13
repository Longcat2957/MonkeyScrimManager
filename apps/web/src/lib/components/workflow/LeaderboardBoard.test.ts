import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { GuildLeaderboardSnapshot } from '$lib/contracts/workflow';
import type { ChampionCatalog } from '$lib/datadragon';
import LeaderboardBoard from './LeaderboardBoard.svelte';

const source = readFileSync(new URL('./LeaderboardBoard.svelte', import.meta.url), 'utf8');
const catalog: ChampionCatalog = {
	version: '16.13.1',
	locale: 'ko_KR',
	champions: [],
	byId: {},
	byKey: {},
	byName: {},
	byIdSlug: {}
};
const snapshot: GuildLeaderboardSnapshot = {
	season: { id: '01900000-0000-7000-8000-000000000010', name: '시즌 1' },
	players: [
		{
			id: '390526792496054283',
			name: 'Longcat',
			tag: null,
			profileIconId: null,
			compositeMmr: 1600,
			mainRole: 'JUNGLE',
			isMe: true,
			stats: {
				JUNGLE: { mmr: 1600, games: 10, wins: 6, championId: null }
			}
		}
	]
};

describe('LeaderboardBoard', () => {
	it('renders actual nullable player data without fixture or broken images', () => {
		const { body } = render(LeaderboardBoard, {
			props: { catalog, snapshot, loadError: null }
		});

		expect(body).toContain('시즌 1 · 1명');
		expect(body).toContain('Longcat');
		expect(body).toContain('Riot ID 비공개');
		expect(body).toContain('6승 4패');
		expect(body).toContain('기록 없음');
		expect(body).not.toContain('/img/profileicon/');
		expect(source).not.toContain('user-tamsol');
		expect(source).not.toContain('const players:');
	});

	it('distinguishes no active season from a request failure', () => {
		const empty = render(LeaderboardBoard, {
			props: { catalog, snapshot: { season: null, players: [] }, loadError: null }
		});
		const failed = render(LeaderboardBoard, {
			props: { catalog, snapshot: null, loadError: '리더보드 조회 실패' }
		});

		expect(empty.body).toContain('현재 진행 중인 시즌이 없습니다.');
		expect(failed.body).toContain('리더보드 조회 실패');
	});
});
