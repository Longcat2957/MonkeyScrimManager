import { mockScrimPickBanResultInputGames } from './scrimPickBanResultInputFixtures';
import type { MockAuctionTeam } from './workflow';
import type { Lane } from './workflow';
import type {
	AuctionBracketMatchSnapshot,
	AuctionBracketSetupSnapshot
} from '$lib/realtime/auction';

export type MockAuctionFormat = 'TEN_PERSON' | 'TWENTY_PERSON';

export interface MockAuctionTournamentRoute {
	id: string;
	format: MockAuctionFormat;
	participantCount: 10 | 20;
	teamCount: 2 | 4;
}

export interface MockAuctionRecruitmentRoute extends MockAuctionTournamentRoute {
	nextTournamentId: string;
}

const tenPersonTournamentIds = ['a10-118', 'a10-117', 'a10-116', 'a10-115'] as const;
const twentyPersonTournamentIds = ['a20-118', 'a20-117', 'a20-116', 'a20-115', 'a-118'] as const;

export const mockAuctionTournamentRoutes: MockAuctionTournamentRoute[] = [
	...tenPersonTournamentIds.map((id) => ({
		id,
		format: 'TEN_PERSON' as const,
		participantCount: 10 as const,
		teamCount: 2 as const
	})),
	...twentyPersonTournamentIds.map((id) => ({
		id,
		format: 'TWENTY_PERSON' as const,
		participantCount: 20 as const,
		teamCount: 4 as const
	}))
];

export const mockAuctionRecruitmentRoutes: MockAuctionRecruitmentRoute[] = [
	{
		id: 'ar10-42',
		format: 'TEN_PERSON',
		participantCount: 10,
		teamCount: 2,
		nextTournamentId: 'a10-118'
	},
	{
		id: 'ar20-42',
		format: 'TWENTY_PERSON',
		participantCount: 20,
		teamCount: 4,
		nextTournamentId: 'a20-118'
	}
];

export function findMockAuctionTournament(id: string) {
	return mockAuctionTournamentRoutes.find((item) => item.id === id) ?? null;
}

export function findMockAuctionRecruitment(id: string) {
	return mockAuctionRecruitmentRoutes.find((item) => item.id === id) ?? null;
}

export function createMockBracketSetupSnapshot(
	teams: MockAuctionTeam[]
): AuctionBracketSetupSnapshot {
	return {
		revision: 4,
		locked: false,
		assignments: {
			A_0: teams[0]?.id ?? null,
			A_1: teams[1]?.id ?? null,
			B_0: teams[2]?.id ?? null,
			B_1: teams[3]?.id ?? null
		}
	};
}

export function createMockBracketMatches(teams: MockAuctionTeam[]): AuctionBracketMatchSnapshot[] {
	const lineupByTeam = Object.fromEntries(
		teams.map((team) => [
			team.id,
			Object.fromEntries(team.members.map((member) => [member.role, member.id]))
		])
	);

	const championByLane: Record<Lane, string> = {
		TOP: 'Gnar',
		JUNGLE: 'Viego',
		MID: 'Ahri',
		BOTTOM: 'Ezreal',
		SUPPORT: 'Thresh'
	};
	const game = (
		matchId: string,
		setNumber: number,
		leftTeam: MockAuctionTeam | undefined,
		rightTeam: MockAuctionTeam | undefined
	) => {
		const source = mockScrimPickBanResultInputGames[Math.min(setNumber - 1, 1)]!;
		return {
			...source,
			id: `${matchId}-set-${setNumber}`,
			label: `${setNumber}세트`,
			bans: { BLUE: [...source.bans.BLUE], RED: [...source.bans.RED] },
			picks: Object.fromEntries(
				[leftTeam, rightTeam].flatMap((team) =>
					(team?.members ?? []).map((member) => [member.id, championByLane[member.role]])
				)
			)
		};
	};

	if (teams.length === 2) {
		return [
			{
				id: 'final',
				label: '결승',
				round: 'FINAL',
				bestOf: 3,
				leftTeamId: teams[0]?.id ?? null,
				rightTeamId: teams[1]?.id ?? null,
				leftScore: 1,
				rightScore: 0,
				winnerTeamId: null,
				lineupsLocked: true,
				resultEntered: false,
				revision: 7,
				lineups: lineupByTeam,
				games: [game('final', 1, teams[0], teams[1])]
			}
		];
	}

	const semiA = {
		id: 'semi-a',
		label: '준결승 A',
		round: 'SEMIFINAL' as const,
		bestOf: 3 as const,
		leftTeamId: teams[0]?.id ?? null,
		rightTeamId: teams[1]?.id ?? null,
		leftScore: 2,
		rightScore: 1,
		winnerTeamId: teams[0]?.id ?? null,
		lineupsLocked: true,
		resultEntered: true,
		revision: 11,
		lineups: lineupByTeam,
		games: [
			game('semi-a', 1, teams[0], teams[1]),
			game('semi-a', 2, teams[0], teams[1]),
			game('semi-a', 3, teams[0], teams[1])
		]
	};
	const semiB = {
		id: 'semi-b',
		label: '준결승 B',
		round: 'SEMIFINAL' as const,
		bestOf: 3 as const,
		leftTeamId: teams[2]?.id ?? null,
		rightTeamId: teams[3]?.id ?? null,
		leftScore: 0,
		rightScore: 2,
		winnerTeamId: teams[3]?.id ?? null,
		lineupsLocked: true,
		resultEntered: true,
		revision: 9,
		lineups: lineupByTeam,
		games: [game('semi-b', 1, teams[2], teams[3]), game('semi-b', 2, teams[2], teams[3])]
	};

	return [
		semiA,
		semiB,
		{
			id: 'final',
			label: '결승',
			round: 'FINAL',
			bestOf: 3,
			leftTeamId: semiA.winnerTeamId,
			rightTeamId: semiB.winnerTeamId,
			leftScore: 1,
			rightScore: 0,
			winnerTeamId: null,
			lineupsLocked: true,
			resultEntered: false,
			revision: 6,
			lineups: lineupByTeam,
			games: [game('final', 1, teams[0], teams[3])]
		}
	];
}
