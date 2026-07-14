import { z } from 'zod';
import {
	seriesPickBanSidePresetSchema,
	seriesPickBanSideSchema,
	seriesPickBanTeamSchema
} from './seriesPickBan';
import { championPreferenceLaneSchema, userIdSchema } from './workflow';

const uuidSchema = z.uuid();
const timestampSchema = z.iso.datetime({ offset: true });
const nonnegativeIntegerSchema = z.number().int().nonnegative();
const championIdSchema = z
	.string()
	.min(1)
	.max(64)
	.regex(/^[A-Za-z0-9]+$/);

export const seriesResultTeamIdSchema = z.enum(['TEAM_1', 'TEAM_2']);

export const seriesResultGameSchema = z
	.object({
		id: uuidSchema,
		setNumber: z.number().int().min(1).max(5),
		sidePreset: seriesPickBanSidePresetSchema,
		winner: seriesPickBanSideSchema,
		picks: z.record(z.string(), championIdSchema.nullable()),
		bans: z
			.object({
				BLUE: z.array(championIdSchema.nullable()).length(5),
				RED: z.array(championIdSchema.nullable()).length(5)
			})
			.strict(),
		patchVersion: z.string().min(1).max(32).nullable(),
		durationSeconds: nonnegativeIntegerSchema.nullable(),
		mmrChanges: z
			.array(
				z
					.object({
						userId: userIdSchema,
						lane: championPreferenceLaneSchema,
						delta: z.number().finite(),
						mmrBefore: z.number().finite(),
						mmrAfter: z.number().finite()
					})
					.strict()
			)
			.max(10)
	})
	.strict();

export const seriesResultSnapshotSchema = z
	.object({
		series: z
			.object({
				id: uuidSchema,
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(100),
				bestOf: z.union([z.literal(1), z.literal(3), z.literal(5)]),
				hardFearless: z.boolean(),
				status: z.literal('COMPLETED'),
				revision: nonnegativeIntegerSchema,
				sourceRecruitmentId: uuidSchema,
				winningTeam: seriesResultTeamIdSchema.nullable(),
				terminalReason: z.string().min(1).max(100).nullable(),
				startedAt: timestampSchema,
				endedAt: timestampSchema.nullable()
			})
			.strict(),
		teams: z.array(seriesPickBanTeamSchema).length(2),
		games: z.array(seriesResultGameSchema).max(5),
		score: z
			.object({
				team1: nonnegativeIntegerSchema,
				team2: nonnegativeIntegerSchema
			})
			.strict(),
		capabilities: z
			.object({
				canRevertLastGame: z.boolean(),
				canDelete: z.boolean()
			})
			.strict()
	})
	.strict()
	.superRefine((snapshot, context) => {
		const teamIds = snapshot.teams.map((team) => team.team);
		for (const teamId of ['TEAM_1', 'TEAM_2'] as const) {
			if (!teamIds.includes(teamId)) {
				context.addIssue({
					code: 'custom',
					path: ['teams'],
					message: `Missing ${teamId}`
				});
			}
		}
		if (new Set(teamIds).size !== teamIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: 'Result teams must be unique'
			});
		}

		const rosterIds = snapshot.teams.flatMap((team) => team.players.map((player) => player.id));
		if (new Set(rosterIds).size !== rosterIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: 'Result roster players must be unique'
			});
		}

		const seenSetNumbers = new Set<number>();
		for (const [gameIndex, game] of snapshot.games.entries()) {
			if (seenSetNumbers.has(game.setNumber)) {
				context.addIssue({
					code: 'custom',
					path: ['games', gameIndex, 'setNumber'],
					message: 'Result game set numbers must be unique'
				});
			}
			seenSetNumbers.add(game.setNumber);

			const pickIds = Object.keys(game.picks);
			if (
				pickIds.length !== rosterIds.length ||
				pickIds.some((playerId) => !rosterIds.includes(playerId)) ||
				rosterIds.some((playerId) => !(playerId in game.picks))
			) {
				context.addIssue({
					code: 'custom',
					path: ['games', gameIndex, 'picks'],
					message: 'Result game picks must contain every frozen roster player exactly once'
				});
			}

			const changedUserIds = game.mmrChanges.map((change) => change.userId);
			if (new Set(changedUserIds).size !== changedUserIds.length) {
				context.addIssue({
					code: 'custom',
					path: ['games', gameIndex, 'mmrChanges'],
					message: 'Result MMR changes must contain each user at most once'
				});
			}
		}
	});

export type SeriesResultGame = z.infer<typeof seriesResultGameSchema>;
export type SeriesResultSnapshot = z.infer<typeof seriesResultSnapshotSchema>;
