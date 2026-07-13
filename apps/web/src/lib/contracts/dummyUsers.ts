import { z } from 'zod';

export const dummyLaneSchema = z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);
export const dummyIdentityKindSchema = z.literal('DUMMY');
export const dummyUserStateSchema = z.enum(['ACTIVE', 'ARCHIVED']);
export const dummyMembershipStatusSchema = z.enum(['ACTIVE', 'INACTIVE']);

export const dummyUserIdSchema = z
	.string()
	.regex(/^dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
export const dummyStableKeySchema = z
	.string()
	.min(1)
	.max(64)
	.regex(/^[a-z0-9][a-z0-9_-]*$/);

export const dummyPrivacySchema = z
	.object({
		publicProfileEnabled: z.boolean(),
		showRiotIds: z.boolean(),
		allowExternalRecordSearch: z.boolean()
	})
	.strict();

export const dummyChampionPreferencesSchema = z
	.object({
		TOP: z.array(z.string().min(1).max(64)).max(10),
		JUNGLE: z.array(z.string().min(1).max(64)).max(10),
		MID: z.array(z.string().min(1).max(64)).max(10),
		BOTTOM: z.array(z.string().min(1).max(64)).max(10),
		SUPPORT: z.array(z.string().min(1).max(64)).max(10)
	})
	.strict();

export const dummyLaneMmrSchema = z
	.object({
		top: z.number().finite().min(0).max(10_000),
		jungle: z.number().finite().min(0).max(10_000),
		mid: z.number().finite().min(0).max(10_000),
		bottom: z.number().finite().min(0).max(10_000),
		support: z.number().finite().min(0).max(10_000)
	})
	.strict();

export const dummyRiotAccountSchema = z
	.object({
		platform: z.string().min(1).max(16),
		routingRegion: z.string().min(1).max(16),
		gameName: z.string().min(1).max(64),
		tagLine: z.string().min(1).max(16),
		profileIconId: z.number().int().min(0).max(2_147_483_647).nullable(),
		mainPosition: z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT', 'FLEX']).nullable()
	})
	.strict();

export const putDummyRiotAccountInputSchema = z
	.object({
		platform: z.string().min(1).max(16),
		routingRegion: z.string().min(1).max(16),
		gameName: z.string().min(1).max(64),
		tagLine: z.string().min(1).max(16),
		profileIconId: z.number().int().min(0).max(2_147_483_647).optional(),
		mainPosition: z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT', 'FLEX']).optional()
	})
	.strict();

export const dummyGuildMembershipSchema = z
	.object({
		guildId: z.string().min(1),
		guildName: z.string().min(1),
		displayName: z.string().min(1).max(100),
		status: dummyMembershipStatusSchema,
		mainLane: dummyLaneSchema.nullable(),
		laneMmr: dummyLaneMmrSchema.nullable()
	})
	.strict();

export const dummyUserSchema = z
	.object({
		userId: dummyUserIdSchema,
		identityKind: dummyIdentityKindSchema,
		stableKey: dummyStableKeySchema,
		displayName: z.string().min(1).max(100),
		avatarSeed: z.string().min(1).max(128).nullable(),
		notes: z.string().max(1000).nullable(),
		state: dummyUserStateSchema,
		revision: z.number().int().nonnegative(),
		privacy: dummyPrivacySchema,
		riotAccount: dummyRiotAccountSchema.nullable(),
		championPreferences: dummyChampionPreferencesSchema,
		guildMemberships: z.array(dummyGuildMembershipSchema),
		groupIds: z.array(z.uuid()),
		createdAt: z.iso.datetime({ offset: true }),
		updatedAt: z.iso.datetime({ offset: true })
	})
	.strict();

export const dummyUsersListDataSchema = z
	.object({
		users: z.array(dummyUserSchema),
		total: z.number().int().nonnegative()
	})
	.strict();

export const dummyGuildCatalogItemSchema = z
	.object({
		guildId: z.string().min(1),
		displayName: z.string().min(1),
		status: z.enum(['INSTALLED', 'SETUP_REQUIRED', 'ACTIVE', 'DISABLED', 'LEFT']),
		activeSeasonId: z.uuid().nullable()
	})
	.strict();

export const dummyGuildCatalogDataSchema = z
	.object({ guilds: z.array(dummyGuildCatalogItemSchema) })
	.strict();

export const dummyGroupMemberSchema = z
	.object({
		userId: dummyUserIdSchema,
		displayName: z.string().min(1).max(100),
		position: z.number().int().positive()
	})
	.strict();

export const dummyGroupSchema = z
	.object({
		id: z.uuid(),
		name: z.string().min(1).max(100),
		description: z.string().max(500).nullable(),
		revision: z.number().int().nonnegative(),
		memberCount: z.number().int().nonnegative(),
		members: z.array(dummyGroupMemberSchema),
		createdAt: z.iso.datetime({ offset: true }),
		updatedAt: z.iso.datetime({ offset: true })
	})
	.strict()
	.superRefine((group, context) => {
		if (group.memberCount !== group.members.length) {
			context.addIssue({
				code: 'custom',
				path: ['memberCount'],
				message: 'Group memberCount must match members length'
			});
		}
	});

export const dummyGroupsListDataSchema = z.object({ groups: z.array(dummyGroupSchema) }).strict();

export const dummyBulkCreateDataSchema = z
	.object({
		users: z.array(dummyUserSchema),
		total: z.number().int().nonnegative()
	})
	.strict();

export const dummyDeleteResultSchema = z.object({ deleted: z.literal(true) }).strict();

const deletionReferenceCountsSchema = z
	.object({
		guildMemberships: z.number().int().nonnegative(),
		recruitmentParticipants: z.number().int().nonnegative(),
		auctionParticipants: z.number().int().nonnegative(),
		gameParticipants: z.number().int().nonnegative(),
		userSessions: z.number().int().nonnegative(),
		discordIdentities: z.number().int().nonnegative(),
		realRiotAccounts: z.number().int().nonnegative()
	})
	.strict();

const deletionRemovableCountsSchema = z
	.object({
		groupMemberships: z.number().int().nonnegative(),
		championPreferences: z.number().int().nonnegative(),
		syntheticRiotAccounts: z.number().int().nonnegative()
	})
	.strict();

export const dummyUserDeletionImpactSchema = z
	.object({
		canDelete: z.boolean(),
		blockingReferences: deletionReferenceCountsSchema,
		removableReferences: deletionRemovableCountsSchema
	})
	.strict();

export const dummyUserDeletionResultSchema = z
	.object({
		deleted: z.literal(true),
		impact: dummyUserDeletionImpactSchema
	})
	.strict();

export const createDummyUserInputSchema = z
	.object({
		stableKey: dummyStableKeySchema,
		displayName: z.string().min(1).max(100),
		avatarSeed: z.string().min(1).max(128).optional(),
		notes: z.string().max(1000).optional(),
		privacy: dummyPrivacySchema.optional(),
		guildMemberships: z
			.array(
				z
					.object({
						guildId: z.string().min(1),
						displayName: z.string().min(1).max(100),
						status: dummyMembershipStatusSchema,
						mainLane: dummyLaneSchema.nullable().optional(),
						laneMmr: dummyLaneMmrSchema.optional()
					})
					.strict()
			)
			.optional(),
		riotAccount: putDummyRiotAccountInputSchema.optional()
	})
	.strict();

export const updateDummyUserInputSchema = z
	.object({
		displayName: z.string().min(1).max(100).optional(),
		avatarSeed: z.string().min(1).max(128).nullable().optional(),
		notes: z.string().max(1000).nullable().optional(),
		privacy: dummyPrivacySchema.optional(),
		expectedRevision: z.number().int().nonnegative()
	})
	.strict();

export const putDummyGuildMembershipInputSchema = z
	.object({
		displayName: z.string().min(1).max(100),
		status: dummyMembershipStatusSchema,
		mainLane: dummyLaneSchema.nullable().optional(),
		laneMmr: dummyLaneMmrSchema.optional()
	})
	.strict();

export const createDummyGroupInputSchema = z
	.object({
		name: z.string().min(1).max(100),
		description: z.string().max(500).optional(),
		memberIds: z.array(dummyUserIdSchema).max(100).optional()
	})
	.strict();

export type DummyLane = z.infer<typeof dummyLaneSchema>;
export type DummyUserState = z.infer<typeof dummyUserStateSchema>;
export type DummyPrivacy = z.infer<typeof dummyPrivacySchema>;
export type DummyLaneMmr = z.infer<typeof dummyLaneMmrSchema>;
export type DummyGuildMembership = z.infer<typeof dummyGuildMembershipSchema>;
export type DummyUser = z.infer<typeof dummyUserSchema>;
export type DummyUsersListData = z.infer<typeof dummyUsersListDataSchema>;
export type DummyGuildCatalogItem = z.infer<typeof dummyGuildCatalogItemSchema>;
export type DummyGuildCatalogData = z.infer<typeof dummyGuildCatalogDataSchema>;
export type DummyGroup = z.infer<typeof dummyGroupSchema>;
export type DummyGroupsListData = z.infer<typeof dummyGroupsListDataSchema>;
export type CreateDummyUserInput = z.infer<typeof createDummyUserInputSchema>;
export type UpdateDummyUserInput = z.infer<typeof updateDummyUserInputSchema>;
export type PutDummyGuildMembershipInput = z.infer<typeof putDummyGuildMembershipInputSchema>;
export type CreateDummyGroupInput = z.infer<typeof createDummyGroupInputSchema>;
export type PutDummyRiotAccountInput = z.infer<typeof putDummyRiotAccountInputSchema>;
export type DummyUserDeletionImpact = z.infer<typeof dummyUserDeletionImpactSchema>;
