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

	it('converts a closed full roster into an auction recruitment draft from a dedicated capability', () => {
		expect(source).toContain('canCreateAuctionRecruitment = $derived(');
		expect(source).toContain('snapshot?.capabilities.canCreateAuctionRecruitment');
		expect(source).toContain('openCreateAuctionRecruitmentDialog');
		expect(source).toContain('createAuctionRecruitmentExpectedStartDate');
		expect(source).toContain('createAuctionRecruitmentExpectedStartTime');
		expect(source).toContain('createAuctionRecruitmentDefaultExpectedStartAt');
		expect(source).toContain('Date.now() + 30 * 60 * 1000');
		expect(source).toContain('예상 시작 시간 (선택)');
		expect(source).toContain('비우면 생성 시점 30분 후로 정합니다.');
		expect(source).toContain('type="date"');
		expect(source).toContain('시간 (24시간)');
		expect(source).toContain('placeholder="23:30"');
		expect(source).toContain('pattern="[0-2][0-9]:[0-5][0-9]"');
		expect(source).toContain('T${localTime}:00');
		expect(source).toContain('createAuctionRecruitmentFromEntryDraft(');
		expect(source).toContain("'entry-draft-create-auction-recruitment'");
		expect(source).toContain('expectedRevision: currentSnapshot.recruitment.revision');
		expect(source).toContain('expectedStartAt');
		expect(source).toContain(
			'await goto(auctionRecruitment.links.draft ?? auctionRecruitment.recruitment.href);'
		);
		expect(source).toContain('경매 모집 전환');
	});

	it('shows the separate participant confirmation DM action only from the server capability', () => {
		expect(source).toContain('canSendParticipantConfirmationDms = $derived(');
		expect(source).toContain('snapshot?.capabilities.canSendParticipantConfirmationDms');
		expect(source).toContain('dispatchRecruitmentParticipantConfirmationDms(');
		expect(source).toContain("'entry-draft-participant-confirmation-dms'");
		expect(source).toContain('참가 확정 DM 발송');
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
