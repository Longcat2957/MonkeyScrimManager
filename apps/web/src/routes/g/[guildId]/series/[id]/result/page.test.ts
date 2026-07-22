import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const loadSource = readFileSync(new URL('./+page.ts', import.meta.url), 'utf8');
const boardSource = readFileSync(
	new URL(
		'../../../../../../lib/components/workflow/SeriesResultLedgerBoard.svelte',
		import.meta.url
	),
	'utf8'
);

describe('series result page integration', () => {
	it('loads the authoritative completed ledger and never renders a local fixture fallback', () => {
		expect(source).toContain("from '$lib/api/seriesResult'");
		expect(source).toContain('fetchSeriesResult(route.guildId, route.seriesId)');
		expect(source).toContain(
			'<SeriesResultLedgerBoard catalog={data.championCatalog} {snapshot} />'
		);
		expect(source).not.toContain('mockScrimPickBanResultInput');
		expect(loadSource).toContain('loadCachedChampionCatalog(fetch');
	});

	it('keeps the ledger layout but maps frozen teams through each confirmed game side preset', () => {
		expect(boardSource).toContain("from '$lib/contracts/seriesResult'");
		expect(source).toContain("from '$lib/workflow/seriesRules'");
		expect(source).toContain(
			'formatSeriesRules(snapshot.series.bestOf, snapshot.series.hardFearless)'
		);
		expect(boardSource).toContain("game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED'");
		expect(boardSource).toContain('mmrChanges');
		expect(boardSource).toContain('snapshot.series.bestOf');
		expect(boardSource).toContain('disabled={!game}');
		expect(source).toContain("terminalReason === 'EARLY_WIN'");
		expect(source).toContain("terminalReason === 'EARLY_DRAW'");
		expect(source).toContain('조기 종료 · 승리 처리');
		expect(source).toContain('조기 종료 · 무승부');
		expect(boardSource).not.toContain('$lib/fixtures');
		expect(boardSource).not.toContain('result-game-1');
	});

	it('keeps completed-series context and navigation in the route header without duplicating it in the ledger', () => {
		expect(source).toContain(
			'<WorkspaceHeader kicker="일반 내전" title="시리즈 결과" description={resultDescription}>'
		);
		expect(source).toContain("resolve('/g/[guildId]/series', { guildId: data.guildId })");
		expect(source).toContain('내전 목록');
		expect(source.indexOf('<WorkspaceHeader')).toBeLessThan(
			source.indexOf('<SeriesResultLedgerBoard')
		);
		expect(boardSource).not.toContain('{snapshot.series.title}');
		expect(boardSource).not.toContain("from '$lib/workflow/seriesRules'");
	});

	it('exposes only server-authorized result actions through confirmed HTTP mutations', () => {
		expect(source).toContain(
			'canRevertLastGame = $derived(Boolean(snapshot?.capabilities.canRevertLastGame))'
		);
		expect(source).toContain('canDelete = $derived(Boolean(snapshot?.capabilities.canDelete))');
		expect(source).toContain('revertLastSeriesPickBanGame(route.guildId, route.seriesId');
		expect(source).toContain('deleteSeries(route.guildId, route.seriesId');
		expect(source).toContain('expectedRevision: currentSnapshot.series.revision');
		expect(source).toContain("idempotencyKeys.keyFor('series-result-revert-last-game', operation)");
		expect(source).toContain("idempotencyKeys.keyFor('series-result-delete', operation)");
		expect(source).toContain('await goto(result.nextHref);');
		expect(source).toContain('isSeriesPickBanRevisionConflict(cause)');
		expect(source).toContain("idempotencyKeys.invalidate('series-result-revert-last-game')");
		expect(source).toContain("idempotencyKeys.invalidate('series-result-delete')");
		expect(source).toContain("await recoverAfterRevisionConflict(cause, 'revertLastGame', route)");
		expect(source).toContain("await recoverAfterRevisionConflict(cause, 'delete', route)");
		expect(source).toContain(
			'서버 최신 상태를 불러왔습니다. 변경된 결과를 확인한 뒤 다시 시도해 주세요.'
		);
		expect(source).toContain(
			'마지막 확정 세트의 결과와 MMR 정산을 되돌린 뒤 픽밴 작업대로 이동합니다.'
		);
		expect(source).toContain('확정된 모든 세트와 MMR 정산을 되돌린 뒤 시리즈를 취소 처리합니다.');
		expect(source).not.toContain('RealtimeEditStateBar');
		expect(source).not.toContain('createBrowserRealtimeSocket');
	});
});
