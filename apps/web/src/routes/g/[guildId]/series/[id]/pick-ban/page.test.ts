import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const loadSource = readFileSync(new URL('./+page.ts', import.meta.url), 'utf8');

describe('pick-ban page integration', () => {
	it('loads the authoritative series snapshot and does not retain route fixtures', () => {
		expect(source).toContain("from '$lib/api/seriesPickBan'");
		expect(source).toContain('fetchSeriesPickBan(route.guildId, route.seriesId)');
		expect(source).toContain('boardTeamsFor(snapshot)');
		expect(source).toContain('boardGameFor(snapshot)');
		expect(source).toContain('data.championCatalog.champions');
		expect(source).not.toContain('mockScrimPickBanResultInput');
		expect(loadSource).toContain('loadCachedChampionCatalog(fetch');
	});

	it('maps roster teams through the current side preset and connects the board to server commands', () => {
		expect(source).toContain("sidePreset === 'DEFAULT' ? 'BLUE' : 'RED'");
		expect(source).toContain('sideSelected: snapshot.currentGame.sideSelected');
		expect(source).toContain("teamFor(snapshot, 'TEAM_1', 'BLUE')");
		expect(source).toContain("teamFor(snapshot, 'TEAM_2', 'BLUE')");
		expect(source).toContain('teamsAreGameSides');
		expect(source).toContain('onDraftOperation={canEdit ? applyDraftOperation : undefined}');
		expect(source).toContain('onSubmit={canSubmit ? submitCurrentGame : undefined}');
		expect(source).toContain('updateSeriesPickBanDraft(route.guildId, route.seriesId');
		expect(source).toContain('submitSeriesPickBanGame(route.guildId, route.seriesId');
		expect(source).toContain('{#key `${snapshot.series.id}:${snapshot.currentGame.setNumber}`}');
		expect(source).not.toContain('${snapshot.currentGame.setNumber}:${snapshot.series.revision}');
	});

	it('keeps confirmed sets in a collapsed disclosure above the unfinished current game', () => {
		expect(source).toContain('ScrimPickBanConfirmedSetsDisclosure');
		expect(source).toContain(
			'<ScrimPickBanConfirmedSetsDisclosure {snapshot} catalog={data.championCatalog} />'
		);
		expect(source.indexOf('<RealtimeEditStateBar')).toBeLessThan(
			source.indexOf('<ScrimPickBanConfirmedSetsDisclosure')
		);
		expect(source.indexOf('<ScrimPickBanConfirmedSetsDisclosure')).toBeLessThan(
			source.indexOf('<ScrimPickBanResultInputBoard')
		);
		expect(source).not.toContain("type PickBanView = 'input' | 'summary'");
		expect(source).not.toContain('selectedSummaryGameFor(snapshot)');
	});

	it('passes immutable series rules into the workbench bulk-input modal', () => {
		expect(source).toContain(
			'formatSeriesRules(snapshot.series.bestOf, snapshot.series.hardFearless)'
		);
		expect(source).toContain('hardFearless={snapshot.series.hardFearless}');
		expect(source).toContain('previousPickChampionIds={previousPickChampionIds(snapshot)}');
		expect(source).not.toContain('ScrimPickBanBulkInput');
	});

	it('does not retain the redundant set-summary tab after moving it into the disclosure', () => {
		expect(source).not.toContain('aria-label="픽밴 화면"');
		expect(source).not.toContain("{ value: 'summary', label: '세트 요약' }");
	});

	it('serializes mutations and reloads authoritative state after a revision conflict', () => {
		expect(source).toContain('const mutationLocked = $derived(isMutating || recoveryRequired)');
		expect(source).toContain(
			'if (!isCurrentRoute(route) || !currentSnapshot || !canEdit || mutationLocked) return;'
		);
		expect(source).toContain('isSeriesPickBanRevisionConflict(cause)');
		expect(source).toContain("await recoverAfterRevisionConflict(cause, 'draft', route);");
		expect(source).toContain('expectedRevision: currentSnapshot.series.revision');
	});

	it('only applies the latest same-route snapshot request', () => {
		expect(source).toContain('let latestSnapshotRequest = 0');
		expect(source).toContain('function beginSnapshotRequest(): number');
		expect(source).toContain(
			'function isLatestSnapshotRequest(route: PickBanRoute, requestNumber: number)'
		);
		expect(source).toContain('if (!isLatestSnapshotRequest(route, requestNumber)) return;');
	});

	it('creates a realtime socket only for editor-capable operators and refetches invalidations', () => {
		expect(source).toContain('if (!canEdit) {');
		expect(source).toContain('seriesPickBanRealtimeTopics(route.guildId, route.seriesId)');
		expect(source).toContain('createBrowserRealtimeSocket({');
		expect(source).toContain('isNewerSeriesPickBanInvalidation(event, snapshot, topics)');
		expect(source).toContain('queueRealtimeSnapshotRefresh(route);');
		expect(source).toContain('{#if canEdit}');
	});

	it('moves editors to the result view when another operator completes the series', () => {
		expect(source).toContain("cause.code !== 'series_not_in_progress'");
		expect(source).toContain('fetchSeriesResult(route.guildId, route.seriesId)');
		expect(source).toContain("resolve('/g/[guildId]/series/[id]/result'");
	});

	it('uses server capabilities and confirmation for destructive lifecycle and latest-game reversal actions', () => {
		expect(source).toContain(
			'canRevertLastGame = $derived(Boolean(snapshot?.capabilities.canRevertLastGame))'
		);
		expect(source).toContain('revertLastSeriesPickBanGame(route.guildId, route.seriesId');
		expect(source).toContain('마지막 결과 되돌리기');
		expect(source).toContain('MMR 반영과 시리즈 점수를 되돌립니다.');
		expect(source).toContain('revertSeries(route.guildId, route.seriesId');
		expect(source).toContain('deleteSeries(route.guildId, route.seriesId');
		expect(source).toContain('확정된 게임 결과와 해당 게임의 MMR 반영도 함께');
		expect(source).toContain('확정된 게임 결과와 MMR 반영을 안전하게 역순으로 되돌린 뒤');
		expect(source).toContain('deleteConfirmationValid');
	});

	it('offers an operator-only early completion flow without recording the unfinished draft', () => {
		expect(source).toContain(
			'canEarlyComplete = $derived(Boolean(snapshot?.capabilities.canEarlyComplete))'
		);
		expect(source).toContain('completeSeriesEarly(route.guildId, route.seriesId');
		expect(source).toContain('시리즈 조기 종료');
		expect(source).toContain('현재 스코어');
		expect(source).toContain('세트에 입력한 픽밴은 결과에 포함되지 않습니다.');
		expect(source).toContain("idempotencyKeys.keyFor('series-early-complete', operation)");
	});
});
