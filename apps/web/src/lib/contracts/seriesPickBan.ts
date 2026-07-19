import { z } from 'zod';
import {
	championPreferenceIdsSchema,
	championPreferenceLaneSchema,
	userIdSchema
} from './workflow';

const uuidSchema = z.uuid();
const timestampSchema = z.iso.datetime({ offset: true });
const nonnegativeIntegerSchema = z.number().int().nonnegative();
const championIdSchema = z
	.string()
	.min(1)
	.max(64)
	.regex(/^[A-Za-z0-9]+$/);
const laneOrder = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;
const seriesBestOfSchema = z.union([z.literal(1), z.literal(3), z.literal(5)]);

export const seriesPickBanSideSchema = z.enum(['BLUE', 'RED']);
export const seriesPickBanSidePresetSchema = z.enum(['DEFAULT', 'SWAPPED']);
export const seriesPickBanTeamIdSchema = z.enum(['TEAM_1', 'TEAM_2']);
export const seriesEarlyCompletionOutcomeSchema = z.enum(['TEAM_1', 'TEAM_2', 'DRAW']);

export const seriesPickBanPlayerSchema = z
	.object({
		id: userIdSchema,
		displayName: z.string().min(1).max(100),
		lane: championPreferenceLaneSchema,
		mmr: z.number().finite(),
		riotTier: z.string().min(1).max(32).nullable(),
		preferredChampionIds: championPreferenceIdsSchema
	})
	.strict();

export const seriesPickBanTeamSchema = z
	.object({
		team: seriesPickBanTeamIdSchema,
		name: z.string().min(1).max(100),
		players: z.array(seriesPickBanPlayerSchema).length(5)
	})
	.strict()
	.superRefine((team, context) => {
		const lanes = team.players.map((player) => player.lane);
		const playerIds = team.players.map((player) => player.id);
		for (const lane of laneOrder) {
			if (!lanes.includes(lane)) {
				context.addIssue({
					code: 'custom',
					path: ['players'],
					message: `Team must contain ${lane}`
				});
			}
		}
		if (new Set(playerIds).size !== playerIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['players'],
				message: 'Team players must be unique'
			});
		}
	});

export const seriesPickBanCurrentGameSchema = z
	.object({
		setNumber: z.number().int().min(1).max(5),
		sidePreset: seriesPickBanSidePresetSchema,
		sideSelected: z.boolean(),
		winner: seriesPickBanSideSchema.nullable(),
		picks: z.record(z.string(), championIdSchema.nullable()),
		bans: z
			.object({
				BLUE: z.array(championIdSchema.nullable()).length(5),
				RED: z.array(championIdSchema.nullable()).length(5)
			})
			.strict(),
		patchVersion: z.string().min(1).max(32).nullable()
	})
	.strict();

export const seriesPickBanConfirmedGameSchema = seriesPickBanCurrentGameSchema.extend({
	winner: seriesPickBanSideSchema
});

const setPickOperationSchema = z
	.object({
		type: z.literal('SET_PICK'),
		playerId: userIdSchema,
		championId: championIdSchema
	})
	.strict();
const setBanOperationSchema = z
	.object({
		type: z.literal('SET_BAN'),
		side: seriesPickBanSideSchema,
		slot: z.number().int().min(1).max(5),
		championId: championIdSchema
	})
	.strict();

export const seriesPickBanBulkItemSchema = z.discriminatedUnion('type', [
	setPickOperationSchema,
	setBanOperationSchema
]);

export const seriesPickBanDraftOperationSchema = z.discriminatedUnion('type', [
	z.object({ type: z.literal('SET_SIDE'), sidePreset: seriesPickBanSidePresetSchema }).strict(),
	setPickOperationSchema,
	z.object({ type: z.literal('CLEAR_PICK'), playerId: userIdSchema }).strict(),
	setBanOperationSchema,
	z
		.object({
			type: z.literal('CLEAR_BAN'),
			side: seriesPickBanSideSchema,
			slot: z.number().int().min(1).max(5)
		})
		.strict(),
	z
		.object({
			type: z.literal('APPLY_BULK'),
			items: z.array(seriesPickBanBulkItemSchema).min(1).max(20)
		})
		.strict()
]);

export const seriesGameMutationResultSchema = z
	.object({
		series: z
			.object({
				id: uuidSchema,
				status: z.enum(['IN_PROGRESS', 'COMPLETED']),
				revision: nonnegativeIntegerSchema
			})
			.strict(),
		confirmedGameCount: nonnegativeIntegerSchema,
		nextHref: z.string().startsWith('/g/')
	})
	.strict();

export const seriesPickBanSnapshotSchema = z
	.object({
		series: z
			.object({
				id: uuidSchema,
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(100),
				bestOf: seriesBestOfSchema,
				hardFearless: z.boolean(),
				status: z.literal('IN_PROGRESS'),
				revision: nonnegativeIntegerSchema,
				sourceRecruitmentId: uuidSchema,
				updatedAt: timestampSchema
			})
			.strict(),
		teams: z.array(seriesPickBanTeamSchema).length(2),
		currentGame: seriesPickBanCurrentGameSchema,
		games: z.array(seriesPickBanConfirmedGameSchema).max(5),
		capabilities: z
			.object({
				canEdit: z.boolean(),
				canSubmit: z.boolean(),
				canEarlyComplete: z.boolean(),
				canRevertLastGame: z.boolean(),
				canRevert: z.boolean(),
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
				message: 'Teams must be unique'
			});
		}

		const rosterIds = snapshot.teams.flatMap((team) => team.players.map((player) => player.id));
		if (new Set(rosterIds).size !== rosterIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: 'Series roster players must be unique'
			});
		}

		const pickIds = Object.keys(snapshot.currentGame.picks);
		if (
			!snapshot.currentGame.sideSelected &&
			(snapshot.currentGame.winner !== null ||
				Object.values(snapshot.currentGame.picks).some(Boolean) ||
				[...snapshot.currentGame.bans.BLUE, ...snapshot.currentGame.bans.RED].some(Boolean))
		) {
			context.addIssue({
				code: 'custom',
				path: ['currentGame', 'sideSelected'],
				message: 'Current game side must be selected before pick-ban input'
			});
		}
		if (
			pickIds.length !== rosterIds.length ||
			pickIds.some((playerId) => !rosterIds.includes(playerId)) ||
			rosterIds.some((playerId) => !(playerId in snapshot.currentGame.picks))
		) {
			context.addIssue({
				code: 'custom',
				path: ['currentGame', 'picks'],
				message: 'Current game picks must contain every frozen roster player exactly once'
			});
		}

		for (const [index, game] of snapshot.games.entries()) {
			if (!game.sideSelected) {
				context.addIssue({
					code: 'custom',
					path: ['games', index, 'sideSelected'],
					message: 'Confirmed game side must be selected'
				});
			}
			const gamePickIds = Object.keys(game.picks);
			if (
				gamePickIds.length !== rosterIds.length ||
				gamePickIds.some((playerId) => !rosterIds.includes(playerId)) ||
				rosterIds.some((playerId) => !(playerId in game.picks))
			) {
				context.addIssue({
					code: 'custom',
					path: ['games', index, 'picks'],
					message: 'Confirmed game picks must contain every frozen roster player exactly once'
				});
			}
		}
	});

export type SeriesPickBanSnapshot = z.infer<typeof seriesPickBanSnapshotSchema>;
export type SeriesPickBanDraftOperation = z.infer<typeof seriesPickBanDraftOperationSchema>;
export type SeriesPickBanBulkItem = z.infer<typeof seriesPickBanBulkItemSchema>;
export type SeriesEarlyCompletionOutcome = z.infer<typeof seriesEarlyCompletionOutcomeSchema>;
export type SeriesGameMutationResult = z.infer<typeof seriesGameMutationResultSchema>;
