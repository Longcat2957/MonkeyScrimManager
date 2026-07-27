import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(
	new URL('./AuctionTournamentBidDraftWorkbench.svelte', import.meta.url),
	'utf8'
);

describe('AuctionTournamentBidDraftWorkbench', () => {
	it('resets the bid timer after a direct bid-input change', () => {
		expect(source).toContain('const nextValue = event.currentTarget.value;');
		expect(source).toContain('bidInputs = { ...bidInputs, [team.id]: nextValue };');
		expect(source).toContain('resetScoreTimer();');
	});

	it('offers a direct team placement for an unsold candidate while no target is active', () => {
		expect(source).toContain('function canAssignUnsoldToTeam');
		expect(source).toContain("kind: 'ASSIGN_UNSOLD'");
		expect(source).toContain('onclick={() => assignUnsoldMemberToTeam(team, member.userId)}');
	});

	it('offers queued-candidate reshuffle through the draft command endpoint', () => {
		expect(source).toContain('function reshuffleCandidates');
		expect(source).toContain("kind: 'RESHUFFLE_CANDIDATES'");
		expect(source).toContain('순서 섞기');
	});

	it('offers a confirmed reset to the auction initial state', () => {
		expect(source).toContain('function resetToInitialState');
		expect(source).toContain("kind: 'RESET_TO_INITIAL'");
		expect(source).toContain('경매 초기화');
	});

	it('exposes the bid countdown as a progress bar', () => {
		expect(source).toContain('role="progressbar"');
		expect(source).toContain('aria-valuenow={scoreTimerRemainingMs}');
		expect(source).toContain('aria-valuetext={`${scoreTimerSeconds}초 남음`}');
	});
});
