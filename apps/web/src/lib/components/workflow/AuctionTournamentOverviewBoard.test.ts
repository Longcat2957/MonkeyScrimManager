import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import {
	auctionTournamentResultSnapshotSchema,
	type AuctionTournamentResultSnapshot
} from '$lib/contracts/auctionTournamentResult';
import AuctionTournamentOverviewBoard from './AuctionTournamentOverviewBoard.svelte';

const source = readFileSync(
	new URL('./AuctionTournamentOverviewBoard.svelte', import.meta.url),
	'utf8'
);
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const teamOneId = '01900000-0000-7000-8000-000000000611';
const teamTwoId = '01900000-0000-7000-8000-000000000612';

function snapshot(
	overrides: Partial<AuctionTournamentResultSnapshot> = {}
): AuctionTournamentResultSnapshot {
	return {
		tournament: {
			id: tournamentId,
			title: '금요 경매 내전',
			displayNumber: 42,
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			status: 'COMPLETED',
			revision: 7,
			championTeamId: teamOneId,
			href: `/g/${guildId}/auction/tournaments/${tournamentId}`
		},
		teams: [
			{
				id: teamOneId,
				teamIndex: 1,
				teamName: '1팀',
				initialPoints: 1000,
				currentPoints: 720,
				spentPoints: 280,
				members: [
					{
						teamMemberId: '01900000-0000-7000-8000-000000000621',
						userId: '1525847677542207548',
						displayName: 'Longcat',
						declaredLane: 'MID',
						acquiredVia: 'CAPTAIN',
						acquisitionPoints: null
					}
				]
			},
			{
				id: teamTwoId,
				teamIndex: 2,
				teamName: '2팀',
				initialPoints: 1000,
				currentPoints: 710,
				spentPoints: 290,
				members: [
					{
						teamMemberId: '01900000-0000-7000-8000-000000000622',
						userId: '1525847677542207549',
						displayName: 'Nyan',
						declaredLane: 'TOP',
						acquiredVia: 'BID',
						acquisitionPoints: 290
					}
				]
			}
		],
		matches: [
			{
				id: '01900000-0000-7000-8000-000000000631',
				matchKey: 'FINAL',
				bestOf: 3,
				hardFearless: false,
				status: 'COMPLETED',
				leftTeamId: teamOneId,
				rightTeamId: teamTwoId,
				leftScore: 2,
				rightScore: 1,
				winnerTeamId: teamOneId,
				lineupsLocked: false,
				lineups: [],
				games: []
			}
		],
		players: [
			{
				userId: '1525847677542207548',
				displayName: 'Longcat',
				teamId: teamOneId,
				games: 3,
				wins: 2,
				losses: 1,
				mmrDelta: 18
			},
			{
				userId: '1525847677542207549',
				displayName: 'Nyan',
				teamId: teamTwoId,
				games: 3,
				wins: 1,
				losses: 2,
				mmrDelta: null
			}
		],
		permissions: { canRevertResults: true, canRecoverBracket: false },
		...overrides
	};
}

describe('AuctionTournamentOverviewBoard', () => {
	it('renders only authoritative final scores, points, and MMR values', () => {
		const { body } = render(AuctionTournamentOverviewBoard, { props: { snapshot: snapshot() } });

		expect(body).toContain('1팀 우승');
		expect(body).toContain('2');
		expect(body).toContain('280P');
		expect(body).toContain('+18');
		expect(body).toContain('미반영');
		expect(body).not.toContain('결과 입력 되돌리기');
		expect(source).not.toContain('mockAuction');
		expect(source).not.toContain('teamMmrDelta');
	});

	it('keeps incomplete brackets and unavailable MMR distinct from completed data', () => {
		const partial = snapshot({
			tournament: {
				...snapshot().tournament,
				status: 'IN_GAME',
				championTeamId: null
			},
			matches: [
				{
					...snapshot().matches[0]!,
					status: 'LINEUP_PENDING',
					rightTeamId: null,
					winnerTeamId: null,
					leftScore: 0,
					rightScore: 0
				}
			]
		});

		const { body } = render(AuctionTournamentOverviewBoard, { props: { snapshot: partial } });

		expect(body).toContain('라인업 대기');
		expect(body).toContain('팀 대기');
		expect(body).not.toContain('1팀 우승');
	});

	it('uses confirmed lineup and game data for compact match details', () => {
		const detailed = snapshot({
			tournament: {
				...snapshot().tournament,
				status: 'IN_GAME',
				championTeamId: null
			},
			matches: [
				{
					...snapshot().matches[0]!,
					status: 'IN_PROGRESS',
					leftScore: 1,
					rightScore: 0,
					winnerTeamId: null,
					lineupsLocked: true,
					lineups: [
						{
							teamId: teamOneId,
							teamMemberId: '01900000-0000-7000-8000-000000000621',
							userId: '1525847677542207548',
							lane: 'TOP'
						},
						{
							teamId: teamTwoId,
							teamMemberId: '01900000-0000-7000-8000-000000000622',
							userId: '1525847677542207549',
							lane: 'SUPPORT'
						}
					],
					games: [
						{
							id: '01900000-0000-7000-8000-000000000641',
							setNumber: 1,
							sidePreset: 'DEFAULT',
							winner: 'BLUE',
							picks: {
								'1525847677542207548': 'Ahri',
								'1525847677542207549': 'Braum'
							},
							bans: {
								BLUE: ['Aatrox', null, null, null, null],
								RED: ['Akali', null, null, null, null]
							},
							patchVersion: '16.13.1'
						}
					]
				}
			]
		});

		const { body } = render(AuctionTournamentOverviewBoard, { props: { snapshot: detailed } });

		expect(body).toContain('확정 라인업 · 세트 상세');
		expect(body).toContain('1세트 · 1팀 승리');
		expect(body).toContain('Ahri');
		expect(body).toContain('Braum');
		expect(body).toContain('밴 · Aatrox');
		expect(body).toContain('탑');
		expect(body).toContain('서포트');
	});

	it('requires actual locked lineups for nested confirmed game results', () => {
		const lanes = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;
		const base = snapshot();
		const teams = base.teams.map((team, teamIndex) => ({
			...team,
			members: lanes.map((lane, memberIndex) => ({
				teamMemberId: `01900000-0000-7000-8000-${String(710 + teamIndex * 10 + memberIndex).padStart(12, '0')}`,
				userId: `900000000000000${teamIndex + 1}${memberIndex}`,
				displayName: `${team.teamName}-${lane}`,
				declaredLane: 'MID' as const,
				acquiredVia: memberIndex === 0 ? ('CAPTAIN' as const) : ('BID' as const),
				acquisitionPoints: memberIndex === 0 ? null : 100
			}))
		}));
		const lineups = teams.flatMap((team) =>
			team.members.map((member, index) => ({
				teamId: team.id,
				teamMemberId: member.teamMemberId,
				userId: member.userId,
				lane: lanes[index]!
			}))
		);
		const picks = Object.fromEntries(
			lineups.map((lineup, index) => [
				lineup.userId,
				['Aatrox', 'Ahri', 'Ashe', 'Braum', 'Caitlyn', 'Diana', 'Ezreal', 'Fiora', 'Galio', 'Hwei'][
					index
				]!
			])
		);
		const completed = {
			...base,
			teams,
			matches: [
				{
					...base.matches[0]!,
					bestOf: 1 as const,
					leftScore: 1,
					rightScore: 0,
					lineupsLocked: true,
					lineups,
					games: [
						{
							id: '01900000-0000-7000-8000-000000000651',
							setNumber: 1,
							sidePreset: 'DEFAULT' as const,
							winner: 'BLUE' as const,
							picks,
							bans: {
								BLUE: ['Irelia', 'Jinx', 'Kaisa', 'Leona', 'Morgana'],
								RED: ['Nami', 'Ornn', 'Poppy', 'Qiyana', 'Rakan']
							},
							patchVersion: '16.13.1'
						}
					]
				}
			]
		};

		expect(auctionTournamentResultSnapshotSchema.safeParse(completed).success).toBe(true);
		expect(
			auctionTournamentResultSnapshotSchema.safeParse({
				...completed,
				matches: [{ ...completed.matches[0]!, lineups: [] }]
			}).success
		).toBe(false);
		expect(
			auctionTournamentResultSnapshotSchema.safeParse({
				...completed,
				permissions: { canRevertResults: true }
			}).success
		).toBe(false);
	});
});
