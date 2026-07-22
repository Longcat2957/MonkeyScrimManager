import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

describe('entry draft page route binding', () => {
	it('resets and refetches the authoritative snapshot when the recruitment route changes', () => {
		expect(source).toContain('$effect(() => {');
		expect(source).toContain('const route = currentEntryRoute();');
		expect(source).toContain('if (route.key === activeRouteKey) return;');
		expect(source).toContain('resetRouteState();');
		expect(source).toContain('void loadEntryDraft(route);');
		expect(source).toContain('if (!isCurrentRoute(route)) return;');
	});

	it('subscribes to both entry domains and resolves realtime changes through the authoritative snapshot', () => {
		expect(source).toContain("from '$lib/realtime/browserSocket'");
		expect(source).toContain("from '$lib/realtime/entryDraft'");
		expect(source).toContain('topics: topics.all');
		expect(source).toContain(
			'onInvalidate: (event) => handleRealtimeInvalidation(event, route, topics)'
		);
		expect(source).toContain('onResyncRequired: () => queueRealtimeSnapshotRefresh(route)');
		expect(source).toContain('isNewerEntryDraftInvalidation(event, snapshot, topics)');
		expect(source).toContain("recoverAuthoritativeSnapshot(null, route, 'realtime')");
	});

	it('passes the authoritative linked Riot tier through to the entry board', () => {
		expect(source).toContain('riotTier: participant.riotTier');
	});

	it('starts a series only after server authorization and a complete ten-player assignment', () => {
		expect(source).toContain(
			'canCreateSeries = $derived(Boolean(snapshot?.capabilities.canCreateSeries))'
		);
		expect(source).toContain('hasCompleteEntryAssignments(snapshot)');
		expect(source).toContain('createSeriesFromEntryDraft(route.guildId');
		expect(source).toContain('expectedRevision: currentSnapshot.recruitment.revision');
		expect(source).toContain('await goto(series.href);');
		expect(source).toContain('시리즈 시작');
	});

	it('converts a complete entry draft into an auction recruitment draft from a dedicated capability', () => {
		expect(source).toContain('canCreateAuctionRecruitment = $derived(');
		expect(source).toContain('snapshot?.capabilities.canCreateAuctionRecruitment');
		expect(source).toContain('createAuctionRecruitmentFromEntryDraft(');
		expect(source).toContain("'entry-draft-create-auction-recruitment'");
		expect(source).toContain('expectedRevision: currentSnapshot.recruitment.revision');
		expect(source).toContain(
			'await goto(auctionRecruitment.links.draft ?? auctionRecruitment.recruitment.href);'
		);
		expect(source).toContain('경매 모집 전환');
	});

	it('requires confirmation before reopening recruitment and explains that the entry draft is preserved', () => {
		expect(source).toContain('let reopenDialogOpen = $state(false)');
		expect(source).toContain('function openReopenDialog()');
		expect(source).toContain('reopenDialogOpen = true;');
		expect(source).toContain('onclick={openReopenDialog}');
		expect(source).toContain('<Dialog.Root bind:open={reopenDialogOpen}>');
		expect(source).toContain('현재 참가자와 엔트리 배치는 유지됩니다.');
		expect(source).toContain('onclick={reopenRecruitment}');
	});
});
