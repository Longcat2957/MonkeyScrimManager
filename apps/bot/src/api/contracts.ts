import { z, type ZodType } from 'zod';

export const discordSnowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
export const uuidSchema = z.uuid();
export const timestampSchema = z.iso.datetime({ offset: true });
export const laneSchema = z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);

export const actorSchema = z
	.object({
		id: discordSnowflakeSchema,
		displayName: z.string().min(1).max(100),
		avatarHash: z.string().min(1).nullable(),
		roleIds: z.array(discordSnowflakeSchema)
	})
	.strict();

export const setupGuildRequestSchema = z
	.object({
		actor: actorSchema,
		guildName: z.string().min(1).max(100),
		guildIconHash: z.string().min(1).nullable(),
		operatorRoleId: discordSnowflakeSchema,
		seasonName: z.string().min(1).max(100)
	})
	.strict();

const currentSeasonSchema = z
	.object({
		id: uuidSchema,
		name: z.string().min(1)
	})
	.strict();

const setupGuildSchema = z
	.object({
		id: discordSnowflakeSchema,
		displayName: z.string().min(1),
		operatorRoleId: discordSnowflakeSchema,
		currentSeason: currentSeasonSchema,
		dashboardHref: z.string().startsWith('/g/')
	})
	.strict();

export const setupGuildDataSchema = z
	.object({
		guild: setupGuildSchema
	})
	.strict();

export const recruitmentParticipantSchema = z
	.object({
		id: discordSnowflakeSchema,
		displayName: z.string().min(1),
		preferredLanes: z.array(laneSchema).max(5)
	})
	.strict();

export const recruitmentSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string().min(1),
		status: z.enum(['OPEN', 'CLOSED', 'CANCELLED']),
		targetCount: z.literal(10),
		revision: z.number().int().nonnegative(),
		channelId: discordSnowflakeSchema.nullable(),
		messageId: discordSnowflakeSchema.nullable(),
		updatedAt: timestampSchema
	})
	.strict();

export const recruitmentSnapshotSchema = z
	.object({
		recruitment: recruitmentSchema,
		participants: z.array(recruitmentParticipantSchema),
		links: z
			.object({
				entry: z.string().startsWith('/g/')
			})
			.strict()
	})
	.strict();

export const createRecruitmentRequestSchema = z
	.object({
		actor: actorSchema
	})
	.strict();

export const bindRecruitmentMessageRequestSchema = z
	.object({
		actor: actorSchema,
		channelId: discordSnowflakeSchema,
		messageId: discordSnowflakeSchema
	})
	.strict();

export const joinRecruitmentRequestSchema = z
	.object({
		actor: actorSchema,
		member: actorSchema
	})
	.strict();

export const actorRequestSchema = z
	.object({
		actor: actorSchema
	})
	.strict();

export const setPreferredLanesRequestSchema = z
	.object({
		actor: actorSchema,
		preferredLanes: z
			.array(laneSchema)
			.min(1)
			.max(5)
			.refine((lanes) => new Set(lanes).size === lanes.length, 'lanes must be unique')
	})
	.strict();

export const responseMetaSchema = z
	.object({
		requestId: uuidSchema
	})
	.strict();

export const apiErrorEnvelopeSchema = z
	.object({
		error: z
			.object({
				code: z.string().min(1),
				message: z.string().min(1),
				requestId: uuidSchema,
				details: z.unknown().optional()
			})
			.strict()
	})
	.strict();

export const apiReadinessDataSchema = z
	.object({
		status: z.enum(['ready', 'degraded', 'not_ready']),
		acceptingRequests: z.boolean(),
		dependencies: z
			.object({
				postgres: z.enum(['available', 'unavailable', 'not_configured']),
				migration: z.enum(['available', 'unavailable', 'not_configured']),
				valkey: z.enum(['available', 'unavailable', 'not_configured'])
			})
			.strict()
	})
	.strict();

export function successEnvelopeSchema<T>(dataSchema: ZodType<T>) {
	return z
		.object({
			data: dataSchema,
			meta: responseMetaSchema
		})
		.strict();
}

export type Lane = z.infer<typeof laneSchema>;
export type Actor = z.infer<typeof actorSchema>;
export type SetupGuildRequest = z.infer<typeof setupGuildRequestSchema>;
export type SetupGuildData = z.infer<typeof setupGuildDataSchema>;
export type RecruitmentParticipant = z.infer<typeof recruitmentParticipantSchema>;
export type Recruitment = z.infer<typeof recruitmentSchema>;
export type RecruitmentSnapshot = z.infer<typeof recruitmentSnapshotSchema>;
export type CreateRecruitmentRequest = z.infer<typeof createRecruitmentRequestSchema>;
export type BindRecruitmentMessageRequest = z.infer<typeof bindRecruitmentMessageRequestSchema>;
export type JoinRecruitmentRequest = z.infer<typeof joinRecruitmentRequestSchema>;
export type ActorRequest = z.infer<typeof actorRequestSchema>;
export type SetPreferredLanesRequest = z.infer<typeof setPreferredLanesRequestSchema>;
export type ApiErrorEnvelope = z.infer<typeof apiErrorEnvelopeSchema>;
export type ApiReadinessData = z.infer<typeof apiReadinessDataSchema>;
