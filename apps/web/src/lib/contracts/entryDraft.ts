import { z } from 'zod';
import { championPreferenceLaneSchema, userIdSchema } from './workflow';

const timestampSchema = z.iso.datetime({ offset: true });
const nonnegativeIntegerSchema = z.number().int().nonnegative();
const seriesBestOfSchema = z.union([z.literal(1), z.literal(3), z.literal(5)]);

export const entryDraftTeamSchema = z.enum(['TEAM_1', 'TEAM_2']);
export const entryDraftSlotSchema = z.enum([
	'TEAM_1_TOP',
	'TEAM_1_JUNGLE',
	'TEAM_1_MID',
	'TEAM_1_BOTTOM',
	'TEAM_1_SUPPORT',
	'TEAM_2_TOP',
	'TEAM_2_JUNGLE',
	'TEAM_2_MID',
	'TEAM_2_BOTTOM',
	'TEAM_2_SUPPORT'
]);

export const entryDraftRecruitmentSchema = z
	.object({
		id: z.uuid(),
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(100),
		status: z.enum(['OPEN', 'CLOSED']),
		targetCount: z.literal(10),
		bestOf: seriesBestOfSchema,
		hardFearless: z.boolean(),
		revision: nonnegativeIntegerSchema,
		updatedAt: timestampSchema
	})
	.strict();

export const entryDraftParticipantSchema = z
	.object({
		id: userIdSchema,
		displayName: z.string().min(1).max(100),
		preferredLanes: z.array(championPreferenceLaneSchema).max(5),
		mainLane: championPreferenceLaneSchema.nullable(),
		mmr: z.number().finite(),
		games: nonnegativeIntegerSchema,
		wins: nonnegativeIntegerSchema,
		profileIconId: nonnegativeIntegerSchema.nullable(),
		riotTier: z.string().min(1).max(32).nullable(),
		championIds: z.array(z.string().min(1).max(64))
	})
	.strict();

export const entryDraftAssignmentSchema = z
	.object({
		participantId: userIdSchema,
		team: entryDraftTeamSchema,
		lane: championPreferenceLaneSchema
	})
	.strict();

export const entryDraftSchema = z
	.object({
		revision: nonnegativeIntegerSchema,
		updatedAt: timestampSchema,
		updatedBy: z
			.object({
				id: userIdSchema,
				displayName: z.string().min(1).max(100)
			})
			.strict()
			.nullable(),
		participants: z.array(entryDraftParticipantSchema).max(10),
		assignments: z.array(entryDraftAssignmentSchema).max(10)
	})
	.strict();

export const entryDraftCapabilitiesSchema = z
	.object({
		canEdit: z.boolean(),
		canReset: z.boolean(),
		canCreateSeries: z.boolean(),
		canCreateAuctionRecruitment: z.boolean(),
		canReopen: z.boolean(),
		canCancel: z.boolean()
	})
	.strict();

export const entryDraftSnapshotSchema = z
	.object({
		recruitment: entryDraftRecruitmentSchema,
		draft: entryDraftSchema,
		capabilities: entryDraftCapabilitiesSchema
	})
	.strict();

export type EntryDraftAssignment = z.infer<typeof entryDraftAssignmentSchema>;
export type EntryDraftParticipant = z.infer<typeof entryDraftParticipantSchema>;
export type EntryDraftSlot = z.infer<typeof entryDraftSlotSchema>;
export type EntryDraftSnapshot = z.infer<typeof entryDraftSnapshotSchema>;
