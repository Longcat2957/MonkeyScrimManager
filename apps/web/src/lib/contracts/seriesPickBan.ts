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

export const seriesPickBanSideSchema = z.enum(['BLUE', 'RED']);
export const seriesPickBanSidePresetSchema = z.enum(['DEFAULT', 'SWAPPED']);
export const seriesPickBanTeamIdSchema = z.enum(['TEAM_1', 'TEAM_2']);

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

export const seriesPickBanSnapshotSchema = z
	.object({
		series: z
			.object({
				id: uuidSchema,
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(100),
				bestOf: z.number().int().min(1).max(5),
				status: z.literal('IN_PROGRESS'),
				revision: nonnegativeIntegerSchema,
				sourceRecruitmentId: uuidSchema,
				updatedAt: timestampSchema
			})
			.strict(),
		teams: z.array(seriesPickBanTeamSchema).length(2),
		currentGame: seriesPickBanCurrentGameSchema,
		games: z.array(z.never()).max(0),
		capabilities: z
			.object({
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
	});

export type SeriesPickBanSnapshot = z.infer<typeof seriesPickBanSnapshotSchema>;
