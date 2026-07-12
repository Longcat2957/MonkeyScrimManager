import type { ScrimPickBanResultInputGame } from '$lib/fixtures/scrimPickBanResultInputFixtures';
import type { Lane } from '$lib/fixtures/workflow';

export type AuctionBracketSlotKey = 'A_0' | 'A_1' | 'B_0' | 'B_1';

export interface AuctionBracketSetupSnapshot {
	revision: number;
	locked: boolean;
	assignments: Record<AuctionBracketSlotKey, string | null>;
}

export interface AuctionBracketMatchSnapshot {
	id: string;
	label: string;
	round: 'SEMIFINAL' | 'FINAL';
	bestOf: 1 | 3 | 5;
	leftTeamId: string | null;
	rightTeamId: string | null;
	leftScore: number;
	rightScore: number;
	winnerTeamId: string | null;
	lineupsLocked: boolean;
	resultEntered: boolean;
	revision: number;
	lineups: Record<string, Partial<Record<Lane, string>>>;
	games: ScrimPickBanResultInputGame[];
}

export type AuctionBracketSetupCommand =
	| {
			kind: 'ASSIGN_TEAM';
			slot: AuctionBracketSlotKey;
			teamId: string;
			expectedRevision: number;
	  }
	| { kind: 'CLEAR_SLOT'; slot: AuctionBracketSlotKey; expectedRevision: number }
	| { kind: 'LOCK_BRACKET'; expectedRevision: number };

export interface AuctionMatchResultCommand {
	kind: 'SUBMIT_GAME_RESULT';
	matchId: string;
	setNumber: number;
	expectedRevision: number;
	game: ScrimPickBanResultInputGame;
}

export function scoreAuctionGames(games: ScrimPickBanResultInputGame[]) {
	let leftScore = 0;
	let rightScore = 0;

	for (const game of games) {
		if (!game.winner) continue;
		const leftWon =
			(game.sidePreset ?? 'DEFAULT') === 'DEFAULT' ? game.winner === 'BLUE' : game.winner === 'RED';
		if (leftWon) leftScore += 1;
		else rightScore += 1;
	}

	return { leftScore, rightScore };
}

export type AuctionDraftCommand =
	| { kind: 'DRAW_NEXT'; expectedRevision: number }
	| { kind: 'MARK_UNSOLD'; memberId: string; expectedRevision: number }
	| {
			kind: 'AWARD_BID';
			memberId: string;
			teamId: string;
			points: number;
			expectedRevision: number;
	  }
	| {
			kind: 'ASSIGN_UNSOLD';
			memberId: string;
			teamId: string;
			expectedRevision: number;
	  }
	| {
			kind: 'ASSIGN_CURRENT';
			memberId: string;
			teamId: string;
			expectedRevision: number;
	  }
	| { kind: 'RESTORE_UNSOLD'; memberId: string; expectedRevision: number }
	| { kind: 'REVERT_LAST'; expectedRevision: number };
