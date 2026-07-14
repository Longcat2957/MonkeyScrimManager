import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const loadSource = readFileSync(new URL('./+page.ts', import.meta.url), 'utf8');

describe('pick-ban page integration', () => {
	it('loads an authoritative series snapshot and does not retain route fixtures or simulated realtime state', () => {
		expect(source).toContain("from '$lib/api/seriesPickBan'");
		expect(source).toContain('fetchSeriesPickBan(route.guildId, route.seriesId)');
		expect(source).toContain('boardTeamsFor(snapshot)');
		expect(source).toContain('boardGameFor(snapshot)');
		expect(source).toContain('data.championCatalog.champions');
		expect(source).not.toContain('mockScrimPickBanResultInput');
		expect(source).not.toContain('RealtimeEditStateBar');
		expect(loadSource).toContain('loadChampionCatalog(fetch');
	});

	it('maps roster teams through the current side preset and keeps result submission disconnected', () => {
		expect(source).toContain("snapshot.currentGame.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED'");
		expect(source).toContain("teamFor(snapshot, 'TEAM_1', 'BLUE')");
		expect(source).toContain("teamFor(snapshot, 'TEAM_2', 'BLUE')");
		expect(source).toContain('teamsAreGameSides');
		expect(source).not.toContain('onSubmit=');
	});

	it('uses server capabilities and an explicit confirmation step for lifecycle mutations', () => {
		expect(source).toContain('canRevert = $derived(Boolean(snapshot?.capabilities.canRevert))');
		expect(source).toContain('canDelete = $derived(Boolean(snapshot?.capabilities.canDelete))');
		expect(source).toContain('revertSeries(route.guildId, route.seriesId');
		expect(source).toContain('deleteSeries(route.guildId, route.seriesId');
		expect(source).toContain('expectedRevision: currentSnapshot.series.revision');
		expect(source).toContain('await goto(result.nextHref);');
		expect(source).toContain('확정된 게임이 없는 시리즈만 엔트리 편성으로 되돌릴 수 있습니다.');
		expect(source).toContain('확정된 게임이 없는 시리즈만 삭제할 수 있습니다.');
	});
});
