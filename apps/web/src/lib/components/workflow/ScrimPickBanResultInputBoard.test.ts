import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(
	new URL('./ScrimPickBanResultInputBoard.svelte', import.meta.url),
	'utf8'
);

describe('ScrimPickBanResultInputBoard server draft mode', () => {
	it('synchronizes authoritative game props without resetting the active input step', () => {
		const syncEffect = source.slice(
			source.indexOf('$effect(() => {'),
			source.indexOf('function captureSnapshot')
		);
		expect(syncEffect).toContain('lastSyncedGameFingerprint');
		expect(syncEffect).toContain('sidePreset = initialGame.sidePreset');
		expect(syncEffect).toContain('picks = { ...initialGame.picks }');
		expect(syncEffect).not.toContain('inputStep =');
	});

	it('delegates semantic draft cells to the supplied server callback instead of local history', () => {
		expect(source).toContain('onDraftOperation?: (operation: SeriesPickBanDraftOperation)');
		expect(source).toContain("{ type: 'SET_BAN', side, slot: index + 1, championId }");
		expect(source).toContain("{ type: 'CLEAR_PICK', playerId }");
		expect(source).toContain('{#if !serverDraft}');
	});

	it('does not render a redundant result input header', () => {
		expect(source).not.toContain('>결과 입력<');
	});

	it('wires editable pick-ban cells to explicit Tab navigation', () => {
		expect(source).toContain('scrimPickBanResultInputCellOrder(pickBanTeams, bans)');
		expect(source).toContain('function navigateInputCell');
		expect(source).toContain('onNavigate={canEditDraft ? navigateInputCell : undefined}');
	});

	it('remaps left and right teams to the selected game sides for local drafts', () => {
		expect(source).toContain('const pickBanTeams = $derived');
		expect(source).toContain("BLUE: teamForSide('BLUE')");
		expect(source).toContain("RED: teamForSide('RED')");
		expect(source).toContain('team={pickBanTeams.BLUE}');
		expect(source).toContain('team={pickBanTeams.RED}');
	});

	it('can opt into client-side duplicate and hard-fearless validation', () => {
		expect(source).toContain('validateDraft?: boolean');
		expect(source).toContain('validateDraft = false');
		expect(source).toContain('(!validateDraft || !hasDraftConflicts)');
		expect(source).toContain('{#if validateDraft && hasDraftConflicts}');
	});

	it('only exposes bulk input after an explicit side selection', () => {
		expect(source).toContain("import ScrimPickBanBulkInput from './ScrimPickBanBulkInput.svelte'");
		expect(source).toContain('let bulkInputOpen = $state(false)');
		expect(source).toContain('let sideSelected = $state');
		expect(source).toContain('{#if serverDraft && sideSelected}');
		expect(source).toContain(
			'if (!canEditDraft || !sideSelected || items.length === 0 || !onDraftOperation) return;'
		);
		expect(source).toContain('bind:open={bulkInputOpen}');
		expect(source).toContain("await onDraftOperation({ type: 'APPLY_BULK', items });");
		expect(source).toContain('hardFearless?: boolean');
		expect(source).toContain('previousPickChampionIds?: readonly string[]');
	});
});
