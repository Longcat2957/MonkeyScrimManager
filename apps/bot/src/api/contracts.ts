import { z, type ZodType } from 'zod';

export const discordSnowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
export const dummyUserIdSchema = z
	.string()
	.regex(/^dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
export const recruitmentParticipantIdSchema = z.union([discordSnowflakeSchema, dummyUserIdSchema]);
export const uuidSchema = z.uuid();
export const timestampSchema = z.iso.datetime({ offset: true });
export const laneSchema = z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);

export const actorSchema = z
	.object({
		id: discordSnowflakeSchema,
		globalDisplayName: z.string().min(1).max(100),
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

export const registerGuildMemberRequestSchema = z
	.object({
		actor: actorSchema,
		gameName: z.string().min(1).max(64),
		tagLine: z.string().min(1).max(16),
		platform: z.literal('KR')
	})
	.strict();

export const registerGuildMemberDataSchema = z
	.object({
		member: z
			.object({
				id: discordSnowflakeSchema,
				displayName: z.string().min(1).max(100),
				status: z.literal('ACTIVE')
			})
			.strict(),
		riotAccount: z
			.object({
				puuid: z.string().min(1).max(256),
				gameName: z.string().min(1).max(64),
				tagLine: z.string().min(1).max(16),
				platform: z.literal('KR'),
				isMain: z.boolean()
			})
			.strict(),
		registration: z.enum(['CREATED', 'EXISTING']),
		accountLink: z.enum(['CREATED', 'EXISTING'])
	})
	.strict();

export const discordCatalogRoleSchema = z
	.object({
		id: discordSnowflakeSchema,
		name: z.string().min(1).max(100),
		position: z.number().int().nonnegative(),
		managed: z.boolean()
	})
	.strict();

export const discordCatalogChannelSchema = z
	.object({
		id: discordSnowflakeSchema,
		name: z.string().min(1).max(100),
		parentName: z.string().min(1).max(100).nullable(),
		position: z.number().int().nonnegative(),
		kind: z.enum(['TEXT', 'ANNOUNCEMENT'])
	})
	.strict();

export const discordCatalogSyncRequestSchema = z
	.object({
		roles: z.array(discordCatalogRoleSchema).max(250),
		channels: z.array(discordCatalogChannelSchema).max(500)
	})
	.strict();

export const discordCatalogSyncDataSchema = z
	.object({
		syncedAt: timestampSchema
	})
	.strict();

export const recruitmentParticipantSchema = z
	.object({
		id: recruitmentParticipantIdSchema,
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
		createdById: recruitmentParticipantIdSchema,
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

const botInstanceIdSchema = z
	.string()
	.min(1)
	.max(128)
	.regex(/^[A-Za-z0-9][A-Za-z0-9._:-]*$/);

export const recruitmentMessageNotificationPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		recruitmentId: uuidSchema,
		snapshot: recruitmentSnapshotSchema
	})
	.strict();

export const botNotificationSchema = z
	.object({
		id: uuidSchema,
		guildId: discordSnowflakeSchema,
		leaseToken: uuidSchema,
		leaseUntil: timestampSchema,
		channelId: discordSnowflakeSchema,
		type: z.string().min(1).max(100),
		payload: z.unknown(),
		discordNonce: z.string().regex(/^[A-Za-z0-9_-]{22}$/),
		attempts: z.number().int().min(1).max(8)
	})
	.strict();

export const claimNotificationsRequestSchema = z
	.object({
		instanceId: botInstanceIdSchema,
		limit: z.number().int().min(1).max(50).optional()
	})
	.strict();

export const claimNotificationsDataSchema = z
	.object({ notifications: z.array(botNotificationSchema).max(50) })
	.strict();

export const acknowledgeNotificationRequestSchema = z
	.object({
		instanceId: botInstanceIdSchema,
		notificationId: uuidSchema,
		leaseToken: uuidSchema,
		discordMessageId: discordSnowflakeSchema
	})
	.strict();

export const acknowledgeNotificationDataSchema = z
	.object({
		acknowledged: z.literal(true),
		deliveredAt: timestampSchema
	})
	.strict();

export const acknowledgeRecruitmentMessageDataSchema = acknowledgeNotificationDataSchema
	.extend({
		recruitment: z.object({ id: uuidSchema, revision: z.number().int().nonnegative() }).strict()
	})
	.strict();

export const nackNotificationRequestSchema = z
	.object({
		instanceId: botInstanceIdSchema,
		notificationId: uuidSchema,
		leaseToken: uuidSchema,
		failure: z.discriminatedUnion('kind', [
			z
				.object({
					kind: z.literal('HTTP'),
					status: z.number().int().min(400).max(599),
					retryAfterMs: z.number().int().positive().max(3_600_000).optional(),
					message: z.string().min(1).max(1024)
				})
				.strict(),
			z
				.object({
					kind: z.literal('TRANSPORT'),
					message: z.string().min(1).max(1024)
				})
				.strict(),
			z
				.object({
					kind: z.literal('DELIVERY_UNKNOWN'),
					message: z.string().min(1).max(1024)
				})
				.strict()
		])
	})
	.strict();

export const nackNotificationDataSchema = z.discriminatedUnion('state', [
	z
		.object({
			state: z.literal('RETRY_SCHEDULED'),
			availableAt: timestampSchema,
			attempts: z.number().int().min(1).max(8)
		})
		.strict(),
	z
		.object({
			state: z.literal('FAILED'),
			failureCode: z.string().min(1),
			attempts: z.number().int().min(1).max(8)
		})
		.strict()
]);

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
export type RegisterGuildMemberRequest = z.infer<typeof registerGuildMemberRequestSchema>;
export type RegisterGuildMemberData = z.infer<typeof registerGuildMemberDataSchema>;
export type DiscordCatalogRole = z.infer<typeof discordCatalogRoleSchema>;
export type DiscordCatalogChannel = z.infer<typeof discordCatalogChannelSchema>;
export type DiscordCatalogSyncRequest = z.infer<typeof discordCatalogSyncRequestSchema>;
export type DiscordCatalogSyncData = z.infer<typeof discordCatalogSyncDataSchema>;
export type RecruitmentParticipant = z.infer<typeof recruitmentParticipantSchema>;
export type Recruitment = z.infer<typeof recruitmentSchema>;
export type RecruitmentSnapshot = z.infer<typeof recruitmentSnapshotSchema>;
export type CreateRecruitmentRequest = z.infer<typeof createRecruitmentRequestSchema>;
export type BindRecruitmentMessageRequest = z.infer<typeof bindRecruitmentMessageRequestSchema>;
export type JoinRecruitmentRequest = z.infer<typeof joinRecruitmentRequestSchema>;
export type ActorRequest = z.infer<typeof actorRequestSchema>;
export type SetPreferredLanesRequest = z.infer<typeof setPreferredLanesRequestSchema>;
export type RecruitmentMessageNotificationPayload = z.infer<
	typeof recruitmentMessageNotificationPayloadSchema
>;
export type BotNotification = z.infer<typeof botNotificationSchema>;
export type ClaimNotificationsRequest = z.infer<typeof claimNotificationsRequestSchema>;
export type ClaimNotificationsData = z.infer<typeof claimNotificationsDataSchema>;
export type AcknowledgeNotificationRequest = z.infer<typeof acknowledgeNotificationRequestSchema>;
export type AcknowledgeNotificationData = z.infer<typeof acknowledgeNotificationDataSchema>;
export type AcknowledgeRecruitmentMessageData = z.infer<
	typeof acknowledgeRecruitmentMessageDataSchema
>;
export type NackNotificationRequest = z.infer<typeof nackNotificationRequestSchema>;
export type NackNotificationData = z.infer<typeof nackNotificationDataSchema>;
export type ApiErrorEnvelope = z.infer<typeof apiErrorEnvelopeSchema>;
export type ApiReadinessData = z.infer<typeof apiReadinessDataSchema>;
