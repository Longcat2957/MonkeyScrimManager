import { z } from 'zod';
import {
	auctionMatchLineupAssignmentSchema,
	auctionMatchLineupMatchSchema,
	auctionMatchLineupPermissionsSchema,
	auctionMatchLineupTeamSchema,
	type AuctionMatchLineupAssignment,
	type AuctionMatchLineupMember,
	type AuctionMatchLineupTeam
} from './auctionMatchLineup';
import { userIdSchema } from './workflow';

const nonnegativeIntegerSchema = z.number().int().nonnegative();
const championIdSchema = z
	.string()
	.min(1)
	.max(64)
	.regex(/^[A-Za-z0-9]+$/);

export const auctionMatchGameSideSchema = z.enum(['BLUE', 'RED']);
export const auctionMatchGameSidePresetSchema = z.enum(['DEFAULT', 'SWAPPED']);

export const auctionMatchGameBansSchema = z
	.object({
		BLUE: z.array(championIdSchema.nullable()).length(5),
		RED: z.array(championIdSchema.nullable()).length(5)
	})
	.strict();

export const auctionMatchConfirmedGameSchema = z
	.object({
		id: z.uuid(),
		setNumber: z.number().int().min(1).max(5),
		sidePreset: auctionMatchGameSidePresetSchema,
		winner: auctionMatchGameSideSchema,
		picks: z.record(userIdSchema, championIdSchema.nullable()),
		bans: auctionMatchGameBansSchema,
		patchVersion: z.string().min(1).max(32).nullable()
	})
	.strict();

export const auctionMatchGameResultPermissionsSchema = auctionMatchLineupPermissionsSchema;

export const auctionMatchGameResultSnapshotSchema = z
	.object({
		match: auctionMatchLineupMatchSchema,
		teams: z.array(auctionMatchLineupTeamSchema).length(2),
		lineups: z.array(auctionMatchLineupAssignmentSchema).max(10),
		games: z.array(auctionMatchConfirmedGameSchema).max(5),
		permissions: auctionMatchGameResultPermissionsSchema
	})
	.passthrough()
	.superRefine((snapshot, context) => {
		const teamById = new Map(snapshot.teams.map((team) => [team.id, team]));
		const expectedTeamIds = new Set([snapshot.match.leftTeamId, snapshot.match.rightTeamId]);
		if (
			teamById.size !== snapshot.teams.length ||
			snapshot.teams.some((team) => !expectedTeamIds.has(team.id)) ||
			expectedTeamIds.size !== teamById.size
		) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: '경기 팀 정보가 양 팀과 일치하지 않습니다.'
			});
		}

		const occupiedSlots = new Set<string>();
		const lineupUserIds = new Set<string>();
		let lineupsMatchRoster = true;
		for (const [index, lineup] of snapshot.lineups.entries()) {
			const team = teamById.get(lineup.teamId);
			const slotKey = `${lineup.teamId}:${lineup.lane}`;
			if (!team) {
				lineupsMatchRoster = false;
				context.addIssue({
					code: 'custom',
					path: ['lineups', index, 'teamId'],
					message: '라인업이 현재 경기 팀을 참조하지 않습니다.'
				});
				continue;
			}
			if (occupiedSlots.has(slotKey)) {
				context.addIssue({
					code: 'custom',
					path: ['lineups', index],
					message: '같은 팀의 동일 라인을 중복 배치할 수 없습니다.'
				});
			}
			occupiedSlots.add(slotKey);

			const member = team.members.find(
				(candidate) => candidate.teamMemberId === lineup.teamMemberId
			);
			if (!member || member.userId !== lineup.userId) {
				lineupsMatchRoster = false;
				context.addIssue({
					code: 'custom',
					path: ['lineups', index],
					message: '라인업 선수가 해당 팀 roster와 일치하지 않습니다.'
				});
			}
			if (lineupUserIds.has(lineup.userId)) {
				context.addIssue({
					code: 'custom',
					path: ['lineups', index],
					message: '같은 선수는 경기 라인업에 한 번만 배치할 수 있습니다.'
				});
			}
			lineupUserIds.add(lineup.userId);
		}

		const hasCompleteLockedLineups =
			snapshot.match.lineupsLocked &&
			snapshot.lineups.length === 10 &&
			occupiedSlots.size === 10 &&
			lineupUserIds.size === 10 &&
			lineupsMatchRoster;
		if ((snapshot.match.lineupsLocked || snapshot.games.length > 0) && !hasCompleteLockedLineups) {
			context.addIssue({
				code: 'custom',
				path: ['lineups'],
				message: '확정된 라인업에는 양 팀의 다섯 라인이 모두 필요합니다.'
			});
		}

		const seenSetNumbers = new Set<number>();
		const previousPickChampionIds = new Set<string>();
		let leftWins = 0;
		let rightWins = 0;
		for (const [index, game] of snapshot.games.entries()) {
			if (seenSetNumbers.has(game.setNumber)) {
				context.addIssue({
					code: 'custom',
					path: ['games', index, 'setNumber'],
					message: '확정된 세트 번호는 중복될 수 없습니다.'
				});
			}
			seenSetNumbers.add(game.setNumber);
			if (game.setNumber > snapshot.match.bestOf) {
				context.addIssue({
					code: 'custom',
					path: ['games', index, 'setNumber'],
					message: '세트 번호가 경기의 최대 세트 수를 초과했습니다.'
				});
			}

			const pickUserIds = Object.keys(game.picks);
			if (
				pickUserIds.length !== lineupUserIds.size ||
				pickUserIds.some((userId) => !lineupUserIds.has(userId)) ||
				[...lineupUserIds].some((userId) => !(userId in game.picks))
			) {
				context.addIssue({
					code: 'custom',
					path: ['games', index, 'picks'],
					message: '게임 픽은 확정된 경기 라인업 전체를 정확히 포함해야 합니다.'
				});
			}

			const championIds = selectedChampionIds(game.picks, game.bans);
			const uniqueChampionIds = new Set(championIds);
			if (championIds.length !== uniqueChampionIds.size) {
				context.addIssue({
					code: 'custom',
					path: ['games', index],
					message: '한 게임에서 같은 챔피언을 중복 선택하거나 밴할 수 없습니다.'
				});
			}
			const pickedChampionIds = Object.values(game.picks).filter(
				(championId): championId is string => championId !== null
			);
			if (
				snapshot.match.hardFearless &&
				pickedChampionIds.some((championId) => previousPickChampionIds.has(championId))
			) {
				context.addIssue({
					code: 'custom',
					path: ['games', index],
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

		if (snapshot.match.leftScore !== leftWins || snapshot.match.rightScore !== rightWins) {
			context.addIssue({
				code: 'custom',
				path: ['match'],
				message: '경기 점수가 확정된 게임 결과와 일치하지 않습니다.'
			});
		}
	});

export const auctionMatchGameResultSubmitCommandSchema = z
	.object({
		expectedRevision: nonnegativeIntegerSchema,
		setNumber: z.number().int().min(1).max(5),
		sidePreset: auctionMatchGameSidePresetSchema,
		winner: auctionMatchGameSideSchema,
		picks: z.record(userIdSchema, championIdSchema.nullable()),
		bans: auctionMatchGameBansSchema,
		patchVersion: z.string().min(1).max(32).nullable()
	})
	.strict();

export const auctionMatchGameResultRevertCommandSchema = z
	.object({ expectedRevision: nonnegativeIntegerSchema })
	.strict();

export type AuctionMatchConfirmedGame = z.infer<typeof auctionMatchConfirmedGameSchema>;
export type AuctionMatchGameBans = z.infer<typeof auctionMatchGameBansSchema>;
export type AuctionMatchGameResultPermissions = z.infer<
	typeof auctionMatchGameResultPermissionsSchema
>;
export type AuctionMatchGameResultRevertCommand = z.infer<
	typeof auctionMatchGameResultRevertCommandSchema
>;
export type AuctionMatchGameResultSnapshot = z.infer<typeof auctionMatchGameResultSnapshotSchema>;
export type AuctionMatchGameResultSubmitCommand = z.infer<
	typeof auctionMatchGameResultSubmitCommandSchema
>;

export interface AuctionMatchResultTeam {
	id: string;
	teamName: string;
	members: Array<AuctionMatchLineupMember & { lane: AuctionMatchLineupAssignment['lane'] }>;
}

export function auctionMatchResultTeams(
	snapshot: AuctionMatchGameResultSnapshot
): AuctionMatchResultTeam[] {
	return [snapshot.match.leftTeamId, snapshot.match.rightTeamId].flatMap((teamId) => {
		const team = snapshot.teams.find((candidate) => candidate.id === teamId);
		if (!team) return [];
		return toResultTeam(team, snapshot.lineups);
	});
}

export interface AuctionMatchGameDraftConflicts {
	duplicateChampionIds: string[];
	hardFearlessChampionIds: string[];
}

export function validateAuctionMatchGameDraft(
	snapshot: AuctionMatchGameResultSnapshot,
	draft: Pick<AuctionMatchGameResultSubmitCommand, 'picks' | 'bans'>
): AuctionMatchGameDraftConflicts {
	const selectedIds = selectedChampionIds(draft.picks, draft.bans);
	const counts = new Map<string, number>();
	for (const championId of selectedIds) {
		counts.set(championId, (counts.get(championId) ?? 0) + 1);
	}
	const duplicateChampionIds = [...counts].flatMap(([championId, count]) =>
		count > 1 ? [championId] : []
	);
	const previouslyPicked = new Set(
		snapshot.games.flatMap((game) =>
			Object.values(game.picks).filter((championId): championId is string => championId !== null)
		)
	);
	const selectedPickIds = Object.values(draft.picks).filter(
		(championId): championId is string => championId !== null
	);
	const hardFearlessChampionIds = snapshot.match.hardFearless
		? [...new Set(selectedPickIds.filter((championId) => previouslyPicked.has(championId)))]
		: [];

	return { duplicateChampionIds, hardFearlessChampionIds };
}

function toResultTeam(
	team: AuctionMatchLineupTeam,
	lineups: AuctionMatchLineupAssignment[]
): AuctionMatchResultTeam {
	return {
		id: team.id,
		teamName: team.teamName,
		members: lineups
			.filter((lineup) => lineup.teamId === team.id)
			.flatMap((lineup) => {
				const member = team.members.find(
					(candidate) => candidate.teamMemberId === lineup.teamMemberId
				);
				return member ? [{ ...member, lane: lineup.lane }] : [];
			})
	};
}

function selectedChampionIds(
	picks: Record<string, string | null>,
	bans: AuctionMatchGameBans
): string[] {
	return [
		...Object.values(picks).filter((championId): championId is string => championId !== null),
		...bans.BLUE.filter((championId): championId is string => championId !== null),
		...bans.RED.filter((championId): championId is string => championId !== null)
	];
}
