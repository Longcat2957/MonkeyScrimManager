import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import type { AuctionTournamentResultSnapshot } from '$lib/contracts/auctionTournamentResult';
import AuctionTournamentBracketResultBoard from './AuctionTournamentBracketResultBoard.svelte';

const source = readFileSync(
	new URL('./AuctionTournamentBracketResultBoard.svelte', import.meta.url),
	'utf8'
);
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const teamIds = [
	'01900000-0000-7000-8000-000000000611',
	'01900000-0000-7000-8000-000000000612',
	'01900000-0000-7000-8000-000000000613',
	'01900000-0000-7000-8000-000000000614'
] as const;

const snapshot: AuctionTournamentResultSnapshot = {
	tournament: {
		id: tournamentId,
		title: '금요 경매 내전',
		displayNumber: 42,
		format: 'TWENTY_PERSON',
		rules: { hardFearless: true, semifinalBestOf: 1, finalBestOf: 3 },
		status: 'IN_GAME',
		revision: 7,
		championTeamId: null,
		href: `/g/${guildId}/auction/tournaments/${tournamentId}`
	},
	teams: teamIds.map((id, index) => ({
		id,
		teamIndex: index + 1,
		teamName: `${index + 1}팀`,
		initialPoints: 1000,
		currentPoints: 700,
		spentPoints: 300,
		members: []
	})),
	matches: [
		{
			id: '01900000-0000-7000-8000-000000000621',
			matchKey: 'SEMIFINAL_A',
			bestOf: 1,
			hardFearless: true,
			status: 'COMPLETED',
			leftTeamId: teamIds[0],
			rightTeamId: teamIds[1],
			leftScore: 1,
			rightScore: 0,
			winnerTeamId: teamIds[0],
			lineupsLocked: false,
			lineups: [],
			games: []
		},
		{
			id: '01900000-0000-7000-8000-000000000622',
			matchKey: 'SEMIFINAL_B',
			bestOf: 1,
			hardFearless: true,
			status: 'IN_PROGRESS',
			leftTeamId: teamIds[2],
			rightTeamId: teamIds[3],
			leftScore: 0,
			rightScore: 0,
			winnerTeamId: null,
			lineupsLocked: false,
			lineups: [],
			games: []
		},
		{
			id: '01900000-0000-7000-8000-000000000623',
			matchKey: 'FINAL',
			bestOf: 3,
			hardFearless: true,
			status: 'LINEUP_PENDING',
			leftTeamId: null,
			rightTeamId: null,
			leftScore: 0,
			rightScore: 0,
			winnerTeamId: null,
			lineupsLocked: false,
			lineups: [],
			games: []
		}
	],
	players: [],
	permissions: { canRevertResults: false, canRecoverBracket: false }
};

describe('AuctionTournamentBracketResultBoard', () => {
	it('renders semifinal cards beside the final card as a read-only bracket', () => {
		const { body } = render(AuctionTournamentBracketResultBoard, {
			props: { snapshot, onOpenMatch: vi.fn() }
		});

		expect(body).toContain('토너먼트 현황');
		expect(body).toContain('준결승 A');
		expect(body).toContain('준결승 B');
		expect(body).toContain('결승');
		expect(body).toContain('결과 확정');
		expect(body).toContain('승자 대기');
		expect(body).toContain('상세보기');
		expect(body).not.toContain('간략히 보기');
		expect(body).not.toContain('결과 입력');
		expect(body).not.toContain('대진 설정');
	});

	it('keeps the lineup detail view collapsed by default and renders it from the result snapshot', () => {
		expect(source).toContain('let expanded = $state(false);');
		expect(source).toContain('aria-expanded={expanded}');
		expect(source).toContain("{expanded ? '간략히 보기' : '상세보기'}");
		expect(source).toContain('id="auction-tournament-bracket-details"');
		expect(source).toContain('<PositionIcon role={slot.lane} size="sm" />');
		expect(source).toContain('assignedLineup(match, team)');
	});

	it('does not restore the retired inline result-input implementation', () => {
		expect(source).not.toContain('ScrimPickBanResultInputBoard');
		expect(source).not.toContain('onSubmitResult');
		expect(source).not.toContain('Dialog.');
	});
});
