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
		recruitmentChannelId: discordSnowflakeSchema,
		entryChannelId: discordSnowflakeSchema.nullable(),
		resultChannelId: discordSnowflakeSchema.nullable(),
		logChannelId: discordSnowflakeSchema.nullable(),
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

export const seriesBestOfSchema = z.union([z.literal(1), z.literal(3), z.literal(5)]);

export const recruitmentSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string().min(1),
		status: z.enum(['OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED']),
		targetCount: z.literal(10),
		bestOf: seriesBestOfSchema,
		hardFearless: z.boolean(),
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
		actor: actorSchema,
		bestOf: seriesBestOfSchema,
		hardFearless: z.boolean(),
		expectedStartAt: timestampSchema
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

export const heartbeatRequestSchema = z
	.object({
		instanceId: botInstanceIdSchema,
		version: z
			.string()
			.min(1)
			.max(100)
			.regex(/^[\x21-\x7e]+$/),
		commitSha: z
			.string()
			.min(1)
			.max(128)
			.regex(/^[A-Za-z0-9._-]+$/)
			.nullable()
	})
	.strict();

export const heartbeatDataSchema = z
	.object({
		accepted: z.literal(true),
		lastHeartbeatAt: timestampSchema
	})
	.strict();

export const recruitmentMessageNotificationPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		recruitmentId: uuidSchema,
		snapshot: recruitmentSnapshotSchema
	})
	.strict();

export const recruitmentParticipantConfirmationDmPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		guildId: discordSnowflakeSchema,
		recipientUserId: discordSnowflakeSchema,
		recruitmentId: uuidSchema,
		recruitmentTitle: z.string().min(1).max(200),
		recruitmentHref: z.string().startsWith('/g/')
	})
	.strict();

export const auctionRecruitmentFormatSchema = z.enum(['TEN_PERSON', 'TWENTY_PERSON']);
export const auctionBestOfSchema = z.union([z.literal(1), z.literal(3), z.literal(5)]);
export const auctionTournamentStatusSchema = z.enum([
	'BIDDING',
	'BRACKET_SETUP',
	'IN_GAME',
	'COMPLETED',
	'CANCELLED'
]);

export const auctionRecruitmentRulesSchema = z
	.object({
		hardFearless: z.boolean(),
		semifinalBestOf: auctionBestOfSchema.nullable(),
		finalBestOf: auctionBestOfSchema
	})
	.strict();

const auctionRecruitmentStatusSchema = z.enum(['OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED']);

export const auctionRecruitmentSummarySchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(200),
		format: auctionRecruitmentFormatSchema,
		participantCount: z.union([z.literal(10), z.literal(20)]),
		joinedParticipantCount: z.number().int().nonnegative(),
		teamCount: z.union([z.literal(2), z.literal(4)]),
		status: auctionRecruitmentStatusSchema,
		revision: z.number().int().nonnegative(),
		channelId: discordSnowflakeSchema.nullable(),
		messageId: discordSnowflakeSchema.nullable(),
		createdById: recruitmentParticipantIdSchema,
		updatedAt: timestampSchema,
		href: z.string().startsWith('/g/'),
		rules: auctionRecruitmentRulesSchema
	})
	.strict();

const legacyAuctionRecruitmentSummarySchema = auctionRecruitmentSummarySchema
	.omit({ rules: true })
	.transform((recruitment) => ({
		...recruitment,
		rules: {
			hardFearless: false,
			semifinalBestOf: recruitment.format === 'TWENTY_PERSON' ? (3 as const) : null,
			finalBestOf: 3 as const
		}
	}));

const auctionRecruitmentSnapshotFields = {
	participants: z
		.array(
			z
				.object({
					id: recruitmentParticipantIdSchema,
					displayName: z.string().min(1).max(100),
					declaredLane: laneSchema.nullable()
				})
				.strict()
		)
		.max(20),
	links: z
		.object({
			draft: z.string().startsWith('/g/').nullable()
		})
		.strict(),
	permissions: z
		.object({
			canManageMembers: z.boolean(),
			canClose: z.boolean(),
			canReopen: z.boolean().optional(),
			canRestore: z.boolean().optional(),
			canSendParticipantConfirmationDms: z.boolean().optional(),
			canCancel: z.boolean()
		})
		.strict()
};

export const auctionRecruitmentSnapshotSchema = z
	.object({ recruitment: auctionRecruitmentSummarySchema, ...auctionRecruitmentSnapshotFields })
	.strict()
	.superRefine(({ recruitment }, context) => {
		const { semifinalBestOf } = recruitment.rules;
		if (recruitment.format === 'TEN_PERSON' && semifinalBestOf !== null) {
			context.addIssue({
				code: 'custom',
				path: ['recruitment', 'rules', 'semifinalBestOf'],
				message: 'TEN_PERSON auction recruitments must not define semifinalBestOf'
			});
		}
		if (recruitment.format === 'TWENTY_PERSON' && semifinalBestOf === null) {
			context.addIssue({
				code: 'custom',
				path: ['recruitment', 'rules', 'semifinalBestOf'],
				message: 'TWENTY_PERSON auction recruitments must define semifinalBestOf'
			});
		}
	});

export const createAuctionRecruitmentRequestSchema = z
	.object({
		actor: actorSchema,
		format: auctionRecruitmentFormatSchema,
		semifinalBestOf: auctionBestOfSchema.nullable(),
		finalBestOf: auctionBestOfSchema,
		hardFearless: z.boolean(),
		expectedStartAt: timestampSchema
	})
	.strict()
	.superRefine(({ format, semifinalBestOf }, context) => {
		if (format === 'TEN_PERSON' && semifinalBestOf !== null) {
			context.addIssue({
				code: 'custom',
				path: ['semifinalBestOf'],
				message: 'TEN_PERSON auction recruitments must not define semifinalBestOf'
			});
		}
		if (format === 'TWENTY_PERSON' && semifinalBestOf === null) {
			context.addIssue({
				code: 'custom',
				path: ['semifinalBestOf'],
				message: 'TWENTY_PERSON auction recruitments must define semifinalBestOf'
			});
		}
	});

export const auctionTournamentCancelPreviewDataSchema = z
	.object({
		tournament: z
			.object({
				id: uuidSchema,
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(200),
				status: auctionTournamentStatusSchema,
				revision: z.number().int().nonnegative()
			})
			.strict()
	})
	.strict();

export const auctionTournamentCancelRequestSchema = z
	.object({
		actor: actorSchema,
		expectedRevision: z.number().int().nonnegative(),
		reason: z
			.string()
			.trim()
			.min(1)
			.refine((value) => [...value].length <= 240, 'reason must contain at most 240 characters')
	})
	.strict();

export const auctionTournamentCancelResultSchema = z
	.object({
		tournamentId: uuidSchema,
		tournamentStatus: auctionTournamentStatusSchema,
		tournamentRevision: z.number().int().nonnegative(),
		sourceRecruitmentId: uuidSchema,
		sourceRecruitmentRevision: z.number().int().nonnegative(),
		reversedGameCount: z.number().int().nonnegative()
	})
	.strict();

export const auctionRecruitmentMemberRequestSchema = z
	.object({
		actor: actorSchema,
		declaredLane: laneSchema.optional()
	})
	.strict();

export const forceAuctionRecruitmentMemberRequestSchema = z
	.object({
		actor: actorSchema,
		member: actorSchema,
		declaredLane: laneSchema.optional()
	})
	.strict();

export const auctionRecruitmentForceCancelRequestSchema = z.object({ actor: actorSchema }).strict();

export const auctionRecruitmentForceCancelResultSchema = z
	.object({
		recruitmentId: uuidSchema,
		recruitmentStatus: z.literal('CANCELLED'),
		recruitmentRevision: z.number().int().nonnegative(),
		tournamentId: uuidSchema.nullable(),
		tournamentStatus: z.literal('CANCELLED').nullable(),
		tournamentRevision: z.number().int().nonnegative().nullable(),
		reversedGameCount: z.number().int().nonnegative()
	})
	.strict()
	.superRefine((result, context) => {
		const tournamentFields = [
			result.tournamentId,
			result.tournamentStatus,
			result.tournamentRevision
		];
		if (
			tournamentFields.some((value) => value === null) &&
			tournamentFields.some((value) => value !== null)
		) {
			context.addIssue({
				code: 'custom',
				path: ['tournamentId'],
				message: 'tournament cancellation fields must be all null or all present'
			});
		}
		if (result.tournamentId === null && result.reversedGameCount !== 0) {
			context.addIssue({
				code: 'custom',
				path: ['reversedGameCount'],
				message: 'a recruitment-only cancellation cannot reverse tournament games'
			});
		}
	});

export const auctionTournamentListStateSchema = z.enum(['active', 'completed']);

export const auctionTournamentListRequestSchema = z
	.object({
		state: auctionTournamentListStateSchema,
		seasonId: uuidSchema.optional(),
		limit: z.number().int().min(1).max(100).optional()
	})
	.strict();

const auctionTournamentListMatchSchema = z
	.object({
		id: uuidSchema,
		label: z.string().min(1).max(100),
		matchKey: z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']),
		status: z.enum(['LINEUP_PENDING', 'READY', 'IN_PROGRESS']),
		confirmedGameCount: z.number().int().nonnegative()
	})
	.strict();

const auctionTournamentListItemSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(200),
		format: auctionRecruitmentFormatSchema,
		participantCount: z.union([z.literal(10), z.literal(20)]),
		teamCount: z.union([z.literal(2), z.literal(4)]),
		status: auctionTournamentStatusSchema,
		revision: z.number().int().nonnegative(),
		activeMemberCount: z.number().int().nonnegative(),
		bidMemberCount: z.number().int().nonnegative(),
		incompleteMatchCount: z.number().int().nonnegative(),
		nextMatch: auctionTournamentListMatchSchema.nullable(),
		championTeam: z
			.object({ id: uuidSchema, name: z.string().min(1).max(100) })
			.strict()
			.nullable(),
		updatedAt: timestampSchema,
		endedAt: timestampSchema.nullable(),
		href: z.string().startsWith('/g/')
	})
	.strict();

export const auctionTournamentListDataSchema = z
	.object({
		tournaments: z.array(auctionTournamentListItemSchema).max(100),
		nextCursor: z.string().min(1).nullable()
	})
	.strict();

const legacyAuctionRecruitmentSnapshotSchema = z
	.object({
		recruitment: legacyAuctionRecruitmentSummarySchema,
		...auctionRecruitmentSnapshotFields
	})
	.strict();

const auctionRecruitmentMessageNotificationPayloadV1Schema = z
	.object({
		schemaVersion: z.literal(1),
		auctionRecruitmentId: uuidSchema,
		snapshot: z.union([auctionRecruitmentSnapshotSchema, legacyAuctionRecruitmentSnapshotSchema])
	})
	.strict();

const auctionRecruitmentMessageNotificationPayloadV2Schema = z
	.object({
		schemaVersion: z.literal(2),
		auctionRecruitmentId: uuidSchema,
		snapshot: auctionRecruitmentSnapshotSchema
	})
	.strict();

export const auctionRecruitmentMessageNotificationPayloadSchema = z
	.union([
		auctionRecruitmentMessageNotificationPayloadV1Schema,
		auctionRecruitmentMessageNotificationPayloadV2Schema
	])
	.superRefine((payload, context) => {
		if (payload.auctionRecruitmentId !== payload.snapshot.recruitment.id) {
			context.addIssue({
				code: 'custom',
				path: ['auctionRecruitmentId'],
				message: 'auctionRecruitmentId must match snapshot.recruitment.id'
			});
		}
	});

const entryConfirmationPlayerSchema = z
	.object({
		id: recruitmentParticipantIdSchema,
		displayName: z.string().min(1).max(100),
		lane: laneSchema
	})
	.strict();

const entryConfirmationTeamSchema = z
	.object({
		team: z.enum(['TEAM_1', 'TEAM_2']),
		name: z.string().min(1).max(100),
		players: z.array(entryConfirmationPlayerSchema).length(5)
	})
	.strict();

export const entryConfirmationSnapshotSchema = z
	.object({
		series: z
			.object({
				id: uuidSchema,
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(200),
				bestOf: seriesBestOfSchema,
				hardFearless: z.boolean()
			})
			.strict(),
		teams: z.array(entryConfirmationTeamSchema).length(2),
		links: z.object({ pickBan: z.string().startsWith('/g/') }).strict()
	})
	.strict();

export const entryConfirmationNotificationPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		seriesId: uuidSchema,
		snapshot: entryConfirmationSnapshotSchema
	})
	.strict();

const auctionMatchLineupConfirmationMatchKeySchema = z.enum([
	'SEMIFINAL_A',
	'SEMIFINAL_B',
	'FINAL'
]);

const auctionMatchLineupConfirmationTournamentSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(200),
		format: auctionRecruitmentFormatSchema,
		hardFearless: z.boolean()
	})
	.strict();

const auctionMatchLineupConfirmationMatchSchema = z
	.object({
		id: uuidSchema,
		matchKey: auctionMatchLineupConfirmationMatchKeySchema,
		label: z.string().min(1).max(100),
		bestOf: auctionBestOfSchema
	})
	.strict();

const auctionMatchLineupConfirmationTeamSchema = z
	.object({
		teamId: uuidSchema,
		name: z.string().min(1).max(100),
		players: z.array(entryConfirmationPlayerSchema).length(5)
	})
	.strict();

export const auctionMatchLineupConfirmationSnapshotSchema = z
	.object({
		tournament: auctionMatchLineupConfirmationTournamentSchema,
		match: auctionMatchLineupConfirmationMatchSchema,
		teams: z.array(auctionMatchLineupConfirmationTeamSchema).length(2),
		links: z.object({ pickBan: z.string().startsWith('/g/') }).strict()
	})
	.strict()
	.superRefine((snapshot, context) => {
		const teamIds = new Set(snapshot.teams.map((team) => team.teamId));
		if (teamIds.size !== snapshot.teams.length) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: 'auction match lineup confirmation teams must be distinct'
			});
		}
	});

export const auctionMatchLineupConfirmationNotificationPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		matchId: uuidSchema,
		snapshot: auctionMatchLineupConfirmationSnapshotSchema
	})
	.strict()
	.superRefine((payload, context) => {
		if (payload.matchId !== payload.snapshot.match.id) {
			context.addIssue({
				code: 'custom',
				path: ['matchId'],
				message: 'matchId must match snapshot.match.id'
			});
		}
	});

const seriesResultTeamSchema = z.enum(['TEAM_1', 'TEAM_2']);
const seriesResultSidePresetSchema = z.enum(['DEFAULT', 'SWAPPED']);
const seriesResultTerminalReasonSchema = z.enum(['SCORE_REACHED', 'EARLY_WIN', 'EARLY_DRAW']);
const seriesResultChampionIdSchema = z
	.string()
	.min(1)
	.max(64)
	.regex(/^[A-Za-z0-9]+$/);

const seriesResultSeriesSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(200),
		bestOf: seriesBestOfSchema,
		hardFearless: z.boolean(),
		winningTeam: seriesResultTeamSchema.nullable(),
		terminalReason: seriesResultTerminalReasonSchema
	})
	.strict();

const seriesResultScoreSchema = z
	.object({
		team1: z.number().int().nonnegative(),
		team2: z.number().int().nonnegative()
	})
	.strict();

const seriesResultLinksSchema = z.object({ result: z.string().startsWith('/g/') }).strict();

const seriesResultCompactGameSchema = z
	.object({
		setNumber: z.number().int().positive(),
		winner: seriesResultTeamSchema
	})
	.strict();

export const seriesResultNotificationV1SnapshotSchema = z
	.object({
		series: seriesResultSeriesSchema,
		score: seriesResultScoreSchema,
		games: z.array(seriesResultCompactGameSchema).min(1).max(5),
		links: seriesResultLinksSchema
	})
	.strict()
	.superRefine(validateSeriesResultOutcome);

const seriesResultNotificationPlayerSchema = z
	.object({
		id: recruitmentParticipantIdSchema,
		displayName: z.string().min(1).max(100),
		lane: laneSchema
	})
	.strict();

const seriesResultNotificationTeamSchema = z
	.object({
		team: seriesResultTeamSchema,
		name: z.string().min(1).max(100),
		players: z.array(seriesResultNotificationPlayerSchema).length(5)
	})
	.strict();

const seriesResultNotificationGamePickSchema = seriesResultNotificationPlayerSchema
	.extend({ championId: seriesResultChampionIdSchema.nullable() })
	.strict();

const seriesResultNotificationGameTeamSchema = z
	.object({
		team: seriesResultTeamSchema,
		name: z.string().min(1).max(100),
		picks: z.array(seriesResultNotificationGamePickSchema).length(5)
	})
	.strict();

const seriesResultNotificationFullGameSchema = z
	.object({
		setNumber: z.number().int().positive(),
		winner: seriesResultTeamSchema,
		sidePreset: seriesResultSidePresetSchema,
		team1: seriesResultNotificationGameTeamSchema,
		team2: seriesResultNotificationGameTeamSchema,
		bans: z
			.object({
				blue: z.array(seriesResultChampionIdSchema.nullable()).length(5),
				red: z.array(seriesResultChampionIdSchema.nullable()).length(5)
			})
			.strict(),
		patchVersion: z.string().min(1).max(32).nullable()
	})
	.strict();

export const seriesResultNotificationV2SnapshotSchema = z
	.object({
		series: seriesResultSeriesSchema,
		teams: z.array(seriesResultNotificationTeamSchema).length(2),
		score: seriesResultScoreSchema,
		games: z.array(seriesResultNotificationFullGameSchema).min(1).max(5),
		links: seriesResultLinksSchema
	})
	.strict()
	.superRefine((snapshot, context) => {
		validateSeriesResultOutcome(snapshot, context);
		validateSeriesResultFullSnapshot(snapshot, context);
	});

// Kept as the compact schema name so existing v1 callers and pending rows remain valid.
export const seriesResultNotificationSnapshotSchema = seriesResultNotificationV1SnapshotSchema;

export const seriesResultNotificationV1PayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		seriesId: uuidSchema,
		snapshot: seriesResultNotificationV1SnapshotSchema
	})
	.strict()
	.superRefine(validateSeriesResultPayload);

export const seriesResultNotificationV2PayloadSchema = z
	.object({
		schemaVersion: z.literal(2),
		seriesId: uuidSchema,
		snapshot: seriesResultNotificationV2SnapshotSchema
	})
	.strict()
	.superRefine(validateSeriesResultPayload);

export const seriesResultNotificationPayloadSchema = z.discriminatedUnion('schemaVersion', [
	seriesResultNotificationV1PayloadSchema,
	seriesResultNotificationV2PayloadSchema
]);

const auctionTournamentResultMatchKeySchema = z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']);

const auctionTournamentResultTournamentSchema = z
	.object({
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(200),
		format: auctionRecruitmentFormatSchema,
		semifinalBestOf: auctionBestOfSchema.nullable(),
		finalBestOf: auctionBestOfSchema,
		hardFearless: z.boolean()
	})
	.strict();

const auctionTournamentResultMatchSchema = z
	.object({
		matchKey: auctionTournamentResultMatchKeySchema,
		label: z.string().min(1).max(100),
		bestOf: auctionBestOfSchema,
		leftTeamName: z.string().min(1).max(100),
		rightTeamName: z.string().min(1).max(100),
		leftScore: z.number().int().nonnegative(),
		rightScore: z.number().int().nonnegative(),
		winnerTeamName: z.string().min(1).max(100)
	})
	.strict();

export const auctionTournamentResultNotificationPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		guildId: discordSnowflakeSchema,
		tournamentId: uuidSchema,
		channelId: discordSnowflakeSchema,
		tournament: auctionTournamentResultTournamentSchema,
		champion: z
			.object({
				teamId: uuidSchema,
				teamName: z.string().min(1).max(100)
			})
			.strict(),
		matches: z.array(auctionTournamentResultMatchSchema).min(1).max(3),
		completedAt: timestampSchema
	})
	.strict()
	.superRefine(validateAuctionTournamentResultPayload);

export const auctionTournamentResultDeleteNotificationPayloadSchema = z
	.object({
		schemaVersion: z.literal(1),
		guildId: discordSnowflakeSchema,
		tournamentId: uuidSchema,
		channelId: discordSnowflakeSchema,
		discordMessageId: discordSnowflakeSchema
	})
	.strict();

function validateAuctionTournamentResultPayload(
	payload: z.infer<typeof auctionTournamentResultNotificationPayloadSchema>,
	context: z.RefinementCtx
): void {
	const { tournament, matches } = payload;
	const expectedMatchKeys: Array<z.infer<typeof auctionTournamentResultMatchKeySchema>> =
		tournament.format === 'TEN_PERSON' ? ['FINAL'] : ['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL'];

	if (
		(tournament.format === 'TEN_PERSON' && tournament.semifinalBestOf !== null) ||
		(tournament.format === 'TWENTY_PERSON' && tournament.semifinalBestOf === null)
	) {
		context.addIssue({
			code: 'custom',
			path: ['tournament', 'semifinalBestOf'],
			message: 'auction tournament format and semifinalBestOf must agree'
		});
	}

	const matchByKey = new Map(matches.map((match) => [match.matchKey, match]));
	if (
		matchByKey.size !== matches.length ||
		expectedMatchKeys.some((matchKey) => !matchByKey.has(matchKey)) ||
		matches.some((match) => !expectedMatchKeys.includes(match.matchKey))
	) {
		context.addIssue({
			code: 'custom',
			path: ['matches'],
			message: 'completed auction tournament matches must match the tournament format'
		});
	}

	for (const [index, match] of matches.entries()) {
		const expectedBestOf =
			match.matchKey === 'FINAL' ? tournament.finalBestOf : tournament.semifinalBestOf;
		if (match.bestOf !== expectedBestOf) {
			context.addIssue({
				code: 'custom',
				path: ['matches', index, 'bestOf'],
				message: 'match best-of must match the tournament rule snapshot'
			});
		}
		if (
			match.winnerTeamName !== match.leftTeamName &&
			match.winnerTeamName !== match.rightTeamName
		) {
			context.addIssue({
				code: 'custom',
				path: ['matches', index, 'winnerTeamName'],
				message: 'match winner must be one of the two match teams'
			});
			continue;
		}

		const winningScore =
			match.winnerTeamName === match.leftTeamName ? match.leftScore : match.rightScore;
		const losingScore =
			match.winnerTeamName === match.leftTeamName ? match.rightScore : match.leftScore;
		const requiredWins = Math.floor(match.bestOf / 2) + 1;
		if (
			winningScore !== requiredWins ||
			winningScore <= losingScore ||
			match.leftScore + match.rightScore > match.bestOf
		) {
			context.addIssue({
				code: 'custom',
				path: ['matches', index],
				message: 'match winner and score must describe a completed best-of result'
			});
		}
	}

	const final = matchByKey.get('FINAL');
	if (final && final.winnerTeamName !== payload.champion.teamName) {
		context.addIssue({
			code: 'custom',
			path: ['champion', 'teamName'],
			message: 'champion team must match the final winner'
		});
	}
}

function validateSeriesResultOutcome(
	snapshot: {
		series: {
			bestOf: 1 | 3 | 5;
			winningTeam: 'TEAM_1' | 'TEAM_2' | null;
			terminalReason: 'SCORE_REACHED' | 'EARLY_WIN' | 'EARLY_DRAW';
		};
		score: { team1: number; team2: number };
		games: ReadonlyArray<{ setNumber: number; winner: 'TEAM_1' | 'TEAM_2' }>;
	},
	context: z.RefinementCtx
): void {
	if (snapshot.games.length > snapshot.series.bestOf) {
		context.addIssue({
			code: 'custom',
			path: ['games'],
			message: 'confirmed game count cannot exceed the series best-of'
		});
	}

	let team1Wins = 0;
	let team2Wins = 0;
	for (const [index, game] of snapshot.games.entries()) {
		if (game.setNumber !== index + 1) {
			context.addIssue({
				code: 'custom',
				path: ['games', index, 'setNumber'],
				message: 'confirmed sets must be sequentially numbered from one'
			});
		}
		if (game.winner === 'TEAM_1') team1Wins += 1;
		else team2Wins += 1;
	}
	if (snapshot.score.team1 !== team1Wins || snapshot.score.team2 !== team2Wins) {
		context.addIssue({
			code: 'custom',
			path: ['score'],
			message: 'series score must match the confirmed set outcomes'
		});
	}

	const requiredWins = Math.floor(snapshot.series.bestOf / 2) + 1;
	if (snapshot.series.terminalReason === 'EARLY_DRAW') {
		if (snapshot.series.winningTeam !== null || snapshot.score.team1 !== snapshot.score.team2) {
			context.addIssue({
				code: 'custom',
				path: ['series'],
				message: 'an early draw must have no winning team and a tied score'
			});
		}
		return;
	}

	if (snapshot.series.winningTeam === null) {
		context.addIssue({
			code: 'custom',
			path: ['series', 'winningTeam'],
			message: 'a completed series result must identify a winning team'
		});
		return;
	}
	if (
		snapshot.series.terminalReason === 'SCORE_REACHED' &&
		(snapshot.series.winningTeam === 'TEAM_1' ? snapshot.score.team1 : snapshot.score.team2) <
			requiredWins
	) {
		context.addIssue({
			code: 'custom',
			path: ['score'],
			message: 'a score-reached result must contain the required winning score'
		});
	}
}

function validateSeriesResultFullSnapshot(
	snapshot: z.infer<typeof seriesResultNotificationV2SnapshotSchema>,
	context: z.RefinementCtx
): void {
	const rosterByTeam = new Map(snapshot.teams.map((team) => [team.team, team]));
	for (const teamId of ['TEAM_1', 'TEAM_2'] as const) {
		if (!rosterByTeam.has(teamId)) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: `series result roster must include ${teamId}`
			});
		}
	}
	if (rosterByTeam.size !== snapshot.teams.length) {
		context.addIssue({
			code: 'custom',
			path: ['teams'],
			message: 'series result roster teams must be unique'
		});
	}

	const rosterPlayerIds = snapshot.teams.flatMap((team) => team.players.map((player) => player.id));
	if (new Set(rosterPlayerIds).size !== rosterPlayerIds.length) {
		context.addIssue({
			code: 'custom',
			path: ['teams'],
			message: 'series result roster players must be unique'
		});
	}
	for (const [teamIndex, team] of snapshot.teams.entries()) {
		const lanes = team.players.map((player) => player.lane);
		if (new Set(lanes).size !== lanes.length) {
			context.addIssue({
				code: 'custom',
				path: ['teams', teamIndex, 'players'],
				message: 'series result roster lanes must be unique per team'
			});
		}
	}

	for (const [gameIndex, game] of snapshot.games.entries()) {
		validateSeriesResultGameTeam(
			game.team1,
			rosterByTeam.get('TEAM_1'),
			'TEAM_1',
			gameIndex,
			'team1',
			context
		);
		validateSeriesResultGameTeam(
			game.team2,
			rosterByTeam.get('TEAM_2'),
			'TEAM_2',
			gameIndex,
			'team2',
			context
		);
	}
}

function validateSeriesResultGameTeam(
	gameTeam: z.infer<typeof seriesResultNotificationGameTeamSchema>,
	rosterTeam: z.infer<typeof seriesResultNotificationTeamSchema> | undefined,
	expectedTeam: 'TEAM_1' | 'TEAM_2',
	gameIndex: number,
	field: 'team1' | 'team2',
	context: z.RefinementCtx
): void {
	if (gameTeam.team !== expectedTeam) {
		context.addIssue({
			code: 'custom',
			path: ['games', gameIndex, field, 'team'],
			message: `${field} must describe ${expectedTeam}`
		});
	}
	if (!rosterTeam) return;
	if (gameTeam.name !== rosterTeam.name) {
		context.addIssue({
			code: 'custom',
			path: ['games', gameIndex, field, 'name'],
			message: `${field} name must match the immutable series roster`
		});
	}

	const rosterById = new Map(rosterTeam.players.map((player) => [player.id, player]));
	const pickIds = gameTeam.picks.map((pick) => pick.id);
	if (
		new Set(pickIds).size !== pickIds.length ||
		pickIds.some((id) => !rosterById.has(id)) ||
		rosterTeam.players.some((player) => !pickIds.includes(player.id))
	) {
		context.addIssue({
			code: 'custom',
			path: ['games', gameIndex, field, 'picks'],
			message: `${field} picks must contain every immutable roster player exactly once`
		});
		return;
	}
	for (const [pickIndex, pick] of gameTeam.picks.entries()) {
		const rosterPlayer = rosterById.get(pick.id);
		if (!rosterPlayer) continue;
		if (pick.displayName !== rosterPlayer.displayName || pick.lane !== rosterPlayer.lane) {
			context.addIssue({
				code: 'custom',
				path: ['games', gameIndex, field, 'picks', pickIndex],
				message: `${field} pick identity must match the immutable series roster`
			});
		}
	}
}

function validateSeriesResultPayload(
	payload: { seriesId: string; snapshot: { series: { id: string } } },
	context: z.RefinementCtx
): void {
	if (payload.seriesId !== payload.snapshot.series.id) {
		context.addIssue({
			code: 'custom',
			path: ['seriesId'],
			message: 'seriesId must match snapshot.series.id'
		});
	}
}

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

export const acknowledgeAuctionRecruitmentMessageDataSchema = acknowledgeNotificationDataSchema
	.extend({
		auctionRecruitment: z
			.object({ id: uuidSchema, revision: z.number().int().nonnegative() })
			.strict()
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
export type AuctionRecruitmentFormat = z.infer<typeof auctionRecruitmentFormatSchema>;
export type AuctionBestOf = z.infer<typeof auctionBestOfSchema>;
export type AuctionRecruitmentRules = z.infer<typeof auctionRecruitmentRulesSchema>;
export type AuctionRecruitmentSnapshot = z.infer<typeof auctionRecruitmentSnapshotSchema>;
export type CreateAuctionRecruitmentRequest = z.infer<typeof createAuctionRecruitmentRequestSchema>;
export type AuctionRecruitmentMemberRequest = z.infer<typeof auctionRecruitmentMemberRequestSchema>;
export type ForceAuctionRecruitmentMemberRequest = z.infer<
	typeof forceAuctionRecruitmentMemberRequestSchema
>;
export type AuctionRecruitmentForceCancelRequest = z.infer<
	typeof auctionRecruitmentForceCancelRequestSchema
>;
export type AuctionRecruitmentForceCancelResult = z.infer<
	typeof auctionRecruitmentForceCancelResultSchema
>;
export type AuctionTournamentListRequest = z.infer<typeof auctionTournamentListRequestSchema>;
export type AuctionTournamentListData = z.infer<typeof auctionTournamentListDataSchema>;
export type AuctionTournamentCancelPreviewData = z.infer<
	typeof auctionTournamentCancelPreviewDataSchema
>;
export type AuctionTournamentCancelRequest = z.infer<typeof auctionTournamentCancelRequestSchema>;
export type AuctionTournamentCancelResult = z.infer<typeof auctionTournamentCancelResultSchema>;
export type BindRecruitmentMessageRequest = z.infer<typeof bindRecruitmentMessageRequestSchema>;
export type JoinRecruitmentRequest = z.infer<typeof joinRecruitmentRequestSchema>;
export type ActorRequest = z.infer<typeof actorRequestSchema>;
export type SetPreferredLanesRequest = z.infer<typeof setPreferredLanesRequestSchema>;
export type RecruitmentMessageNotificationPayload = z.infer<
	typeof recruitmentMessageNotificationPayloadSchema
>;
export type RecruitmentParticipantConfirmationDmPayload = z.infer<
	typeof recruitmentParticipantConfirmationDmPayloadSchema
>;
export type EntryConfirmationSnapshot = z.infer<typeof entryConfirmationSnapshotSchema>;
export type EntryConfirmationNotificationPayload = z.infer<
	typeof entryConfirmationNotificationPayloadSchema
>;
export type AuctionMatchLineupConfirmationSnapshot = z.infer<
	typeof auctionMatchLineupConfirmationSnapshotSchema
>;
export type AuctionMatchLineupConfirmationNotificationPayload = z.infer<
	typeof auctionMatchLineupConfirmationNotificationPayloadSchema
>;
export type SeriesResultNotificationV1Snapshot = z.infer<
	typeof seriesResultNotificationV1SnapshotSchema
>;
export type SeriesResultNotificationV2Snapshot = z.infer<
	typeof seriesResultNotificationV2SnapshotSchema
>;
export type SeriesResultNotificationSnapshot =
	SeriesResultNotificationV1Snapshot | SeriesResultNotificationV2Snapshot;
export type SeriesResultNotificationPayload = z.infer<typeof seriesResultNotificationPayloadSchema>;
export type AuctionTournamentResultNotificationPayload = z.infer<
	typeof auctionTournamentResultNotificationPayloadSchema
>;
export type AuctionTournamentResultDeleteNotificationPayload = z.infer<
	typeof auctionTournamentResultDeleteNotificationPayloadSchema
>;
export type BotNotification = z.infer<typeof botNotificationSchema>;
export type HeartbeatRequest = z.infer<typeof heartbeatRequestSchema>;
export type HeartbeatData = z.infer<typeof heartbeatDataSchema>;
export type ClaimNotificationsRequest = z.infer<typeof claimNotificationsRequestSchema>;
export type ClaimNotificationsData = z.infer<typeof claimNotificationsDataSchema>;
export type AcknowledgeNotificationRequest = z.infer<typeof acknowledgeNotificationRequestSchema>;
export type AcknowledgeNotificationData = z.infer<typeof acknowledgeNotificationDataSchema>;
export type AcknowledgeRecruitmentMessageData = z.infer<
	typeof acknowledgeRecruitmentMessageDataSchema
>;
export type AcknowledgeAuctionRecruitmentMessageData = z.infer<
	typeof acknowledgeAuctionRecruitmentMessageDataSchema
>;
export type NackNotificationRequest = z.infer<typeof nackNotificationRequestSchema>;
export type NackNotificationData = z.infer<typeof nackNotificationDataSchema>;
export type ApiErrorEnvelope = z.infer<typeof apiErrorEnvelopeSchema>;
export type ApiReadinessData = z.infer<typeof apiReadinessDataSchema>;
