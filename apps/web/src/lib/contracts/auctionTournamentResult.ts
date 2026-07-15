import { z } from 'zod';
import {
	auctionBestOfSchema,
	auctionFormatSchema,
	auctionRecruitmentRulesSchema,
	championPreferenceLaneSchema,
	userIdSchema
} from './workflow';
import { auctionMatchConfirmedGameSchema } from './auctionMatchGameResult';
import { auctionMatchLineupAssignmentSchema } from './auctionMatchLineup';

const nonnegativeIntegerSchema = z.number().int().nonnegative();
const revisionSchema = z.number().int().nonnegative();

export const auctionTournamentResultTeamMemberSchema = z
	.object({
		teamMemberId: z.uuid(),
		userId: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable(),
		acquiredVia: z.enum(['CAPTAIN', 'BID', 'MANUAL']),
		acquisitionPoints: nonnegativeIntegerSchema.nullable()
	})
	.strict();

export const auctionTournamentResultTeamSchema = z
	.object({
		id: z.uuid(),
		teamIndex: z.number().int().positive().max(4),
		teamName: z.string().min(1).max(100),
		initialPoints: nonnegativeIntegerSchema,
		currentPoints: nonnegativeIntegerSchema,
		spentPoints: nonnegativeIntegerSchema,
		members: z.array(auctionTournamentResultTeamMemberSchema).max(5)
	})
	.strict();

export const auctionTournamentResultMatchSchema = z
	.object({
		id: z.uuid(),
		matchKey: z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']),
		bestOf: auctionBestOfSchema,
		hardFearless: z.boolean(),
		status: z.enum(['LINEUP_PENDING', 'READY', 'IN_PROGRESS', 'COMPLETED', 'CANCELLED']),
		leftTeamId: z.uuid().nullable(),
		rightTeamId: z.uuid().nullable(),
		leftScore: nonnegativeIntegerSchema,
		rightScore: nonnegativeIntegerSchema,
		winnerTeamId: z.uuid().nullable(),
		lineupsLocked: z.boolean(),
		lineups: z.array(auctionMatchLineupAssignmentSchema).max(10),
		games: z.array(auctionMatchConfirmedGameSchema).max(5)
	})
	.strict();

export const auctionTournamentResultPlayerSchema = z
	.object({
		userId: userIdSchema,
		displayName: z.string().min(1).max(100),
		teamId: z.uuid(),
		games: nonnegativeIntegerSchema,
		wins: nonnegativeIntegerSchema,
		losses: nonnegativeIntegerSchema,
		mmrDelta: z.number().finite().nullable()
	})
	.strict();

export const auctionTournamentResultSnapshotSchema = z
	.object({
		tournament: z
			.object({
				id: z.uuid(),
				title: z.string().min(1).max(200),
				displayNumber: z.number().int().positive(),
				format: auctionFormatSchema,
				rules: auctionRecruitmentRulesSchema,
				status: z.enum(['BIDDING', 'BRACKET_SETUP', 'IN_GAME', 'COMPLETED', 'CANCELLED']),
				revision: revisionSchema,
				championTeamId: z.uuid().nullable(),
				href: z.string().startsWith('/g/')
			})
			.strict(),
		teams: z.array(auctionTournamentResultTeamSchema).min(2).max(4),
		matches: z.array(auctionTournamentResultMatchSchema).max(3),
		players: z.array(auctionTournamentResultPlayerSchema).max(20),
		permissions: z
			.object({ canRevertResults: z.boolean(), canRecoverBracket: z.boolean() })
			.strict()
	})
	.strict()
	.superRefine((snapshot, context) => {
		const teamIds = new Set(snapshot.teams.map((team) => team.id));
		const teamsById = new Map(snapshot.teams.map((team) => [team.id, team]));
		if (snapshot.tournament.championTeamId && !teamIds.has(snapshot.tournament.championTeamId)) {
			context.addIssue({
				code: 'custom',
				path: ['tournament', 'championTeamId'],
				message: '우승 팀이 토너먼트 팀을 참조하지 않습니다.'
			});
		}
		for (const match of snapshot.matches) {
			for (const teamId of [match.leftTeamId, match.rightTeamId, match.winnerTeamId]) {
				if (teamId !== null && !teamIds.has(teamId)) {
					context.addIssue({
						code: 'custom',
						path: ['matches'],
						message: '경기 팀이 토너먼트 팀을 참조하지 않습니다.'
					});
				}
			}

			const matchTeamIds = new Set(
				[match.leftTeamId, match.rightTeamId].filter((teamId): teamId is string => teamId !== null)
			);
			const lineupSlots = new Set<string>();
			const lineupMemberIds = new Set<string>();
			const lineupUserIds = new Set<string>();

			for (const [lineupIndex, lineup] of match.lineups.entries()) {
				const team = teamsById.get(lineup.teamId);
				if (!team || !matchTeamIds.has(lineup.teamId)) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'lineups', lineupIndex, 'teamId'],
						message: '확정 라인업이 현재 경기 팀을 참조하지 않습니다.'
					});
					continue;
				}

				const member = team.members.find(
					(candidate) => candidate.teamMemberId === lineup.teamMemberId
				);
				if (!member || member.userId !== lineup.userId) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'lineups', lineupIndex],
						message: '확정 라인업 선수가 해당 팀 roster와 일치하지 않습니다.'
					});
				}

				const slotKey = `${lineup.teamId}:${lineup.lane}`;
				if (lineupSlots.has(slotKey)) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'lineups', lineupIndex],
						message: '같은 팀의 동일 라인이 확정 라인업에 중복되었습니다.'
					});
				}
				lineupSlots.add(slotKey);

				if (lineupMemberIds.has(lineup.teamMemberId) || lineupUserIds.has(lineup.userId)) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'lineups', lineupIndex],
						message: '같은 선수가 확정 라인업에 중복되었습니다.'
					});
				}
				lineupMemberIds.add(lineup.teamMemberId);
				lineupUserIds.add(lineup.userId);
			}

			if (
				match.lineupsLocked &&
				(matchTeamIds.size !== 2 || match.lineups.length !== 10 || lineupSlots.size !== 10)
			) {
				context.addIssue({
					code: 'custom',
					path: ['matches', match.id, 'lineups'],
					message: '확정된 라인업에는 양 팀의 다섯 라인이 모두 필요합니다.'
				});
			}

			const seenSetNumbers = new Set<number>();
			const previousPickChampionIds = new Set<string>();
			let leftWins = 0;
			let rightWins = 0;
			for (const [gameIndex, game] of match.games.entries()) {
				if (seenSetNumbers.has(game.setNumber)) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'games', gameIndex, 'setNumber'],
						message: '확정된 세트 번호는 중복될 수 없습니다.'
					});
				}
				seenSetNumbers.add(game.setNumber);
				if (game.setNumber > match.bestOf) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'games', gameIndex, 'setNumber'],
						message: '세트 번호가 경기의 최대 세트 수를 초과했습니다.'
					});
				}

				const pickUserIds = Object.keys(game.picks);
				if (
					!match.lineupsLocked ||
					pickUserIds.length !== lineupUserIds.size ||
					pickUserIds.some((userId) => !lineupUserIds.has(userId)) ||
					[...lineupUserIds].some((userId) => !(userId in game.picks))
				) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'games', gameIndex, 'picks'],
						message: '게임 픽은 확정된 경기 라인업 전체를 정확히 포함해야 합니다.'
					});
				}

				const selectedChampionIds = [
					...Object.values(game.picks).filter(
						(championId): championId is string => championId !== null
					),
					...game.bans.BLUE.filter((championId): championId is string => championId !== null),
					...game.bans.RED.filter((championId): championId is string => championId !== null)
				];
				if (selectedChampionIds.length !== new Set(selectedChampionIds).size) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'games', gameIndex],
						message: '한 게임에서 같은 챔피언을 중복 선택하거나 밴할 수 없습니다.'
					});
				}

				const pickedChampionIds = Object.values(game.picks).filter(
					(championId): championId is string => championId !== null
				);
				if (
					match.hardFearless &&
					pickedChampionIds.some((championId) => previousPickChampionIds.has(championId))
				) {
					context.addIssue({
						code: 'custom',
						path: ['matches', match.id, 'games', gameIndex],
						message: '하드 피어리스 경기에서는 이전 확정 게임의 픽을 다시 선택할 수 없습니다.'
					});
				}
				for (const championId of pickedChampionIds) previousPickChampionIds.add(championId);

				const leftWon =
					(game.sidePreset === 'DEFAULT' && game.winner === 'BLUE') ||
					(game.sidePreset === 'SWAPPED' && game.winner === 'RED');
				if (leftWon) leftWins += 1;
				else rightWins += 1;
			}

			if (
				match.games.length > 0 &&
				(match.leftScore !== leftWins || match.rightScore !== rightWins)
			) {
				context.addIssue({
					code: 'custom',
					path: ['matches', match.id],
					message: '경기 점수가 확정된 게임 결과와 일치하지 않습니다.'
				});
			}

			if (
				match.status === 'COMPLETED' &&
				(!match.lineupsLocked || match.games.length === 0 || match.winnerTeamId === null)
			) {
				context.addIssue({
					code: 'custom',
					path: ['matches', match.id],
					message: '완료된 경기에는 확정 라인업, 세트 결과, 승리 팀이 모두 필요합니다.'
				});
			}
		}
		for (const player of snapshot.players) {
			if (!teamIds.has(player.teamId)) {
				context.addIssue({
					code: 'custom',
					path: ['players'],
					message: '정산 선수가 토너먼트 팀을 참조하지 않습니다.'
				});
			}
		}
		const hasSemifinalRule = snapshot.tournament.rules.semifinalBestOf !== null;
		if (snapshot.tournament.format === 'TEN_PERSON' && hasSemifinalRule) {
			context.addIssue({
				code: 'custom',
				path: ['tournament', 'rules', 'semifinalBestOf'],
				message: '10인 경매에는 4강 규칙이 없어야 합니다.'
			});
		}
		if (snapshot.tournament.format === 'TWENTY_PERSON' && !hasSemifinalRule) {
			context.addIssue({
				code: 'custom',
				path: ['tournament', 'rules', 'semifinalBestOf'],
				message: '20인 경매에는 4강 규칙이 필요합니다.'
			});
		}
	});

export type AuctionTournamentResultSnapshot = z.infer<typeof auctionTournamentResultSnapshotSchema>;
export type AuctionTournamentResultTeam = z.infer<typeof auctionTournamentResultTeamSchema>;
export type AuctionTournamentResultTeamMember = z.infer<
	typeof auctionTournamentResultTeamMemberSchema
>;
export type AuctionTournamentResultLineup = z.infer<typeof auctionMatchLineupAssignmentSchema>;
export type AuctionTournamentResultGame = z.infer<typeof auctionMatchConfirmedGameSchema>;
export type AuctionTournamentResultMatch = z.infer<typeof auctionTournamentResultMatchSchema>;
export type AuctionTournamentResultPlayer = z.infer<typeof auctionTournamentResultPlayerSchema>;
