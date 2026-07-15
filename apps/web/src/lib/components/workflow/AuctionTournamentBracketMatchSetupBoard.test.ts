import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import type { AuctionTournamentBracketSnapshot } from '$lib/contracts/auctionTournamentBracket';
import AuctionTournamentBracketMatchSetupBoard from './AuctionTournamentBracketMatchSetupBoard.svelte';

const source = readFileSync(
	new URL('./AuctionTournamentBracketMatchSetupBoard.svelte', import.meta.url),
	'utf8'
);
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const teamOneId = '01900000-0000-7000-8000-000000000611';
const teamTwoId = '01900000-0000-7000-8000-000000000612';

const lockedSnapshot: AuctionTournamentBracketSnapshot = {
	tournament: {
		id: tournamentId,
		title: '금요 경매 내전',
		displayNumber: 42,
		format: 'TEN_PERSON',
		rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
		status: 'BRACKET_SETUP',
		revision: 7,
		href: `/g/${guildId}/auction/tournaments/${tournamentId}`
	},
	teams: [
		{
			id: teamOneId,
			teamIndex: 1,
			teamName: '1팀',
			initialPoints: 1000,
			currentPoints: 720,
			members: []
		},
		{
			id: teamTwoId,
			teamIndex: 2,
			teamName: '2팀',
			initialPoints: 1000,
			currentPoints: 710,
			members: []
		}
	],
	bracket: {
		locked: true,
		slots: [
			{ slotKey: 'A_0', teamId: teamOneId },
			{ slotKey: 'A_1', teamId: teamTwoId },
			{ slotKey: 'B_0', teamId: null },
			{ slotKey: 'B_1', teamId: null }
		],
		matches: []
	},
	permissions: {
		canManageBracket: true,
		canManageMatches: true,
		canRecoverBracket: true,
		canRevertDraft: false
	}
};

describe('AuctionTournamentBracketMatchSetupBoard', () => {
	it('shows a locked bracket without an unavailable recovery action', () => {
		const { body } = render(AuctionTournamentBracketMatchSetupBoard, {
			props: {
				snapshot: lockedSnapshot,
				onCommand: vi.fn(),
				onOpenMatch: vi.fn()
			}
		});

		expect(body).toContain('대진 확정됨');
		expect(body).not.toContain('대진 다시 편성');
		expect(source).not.toContain('UNLOCK_BRACKET');
		expect(source).not.toContain('canRecoverBracket');
	});
});
