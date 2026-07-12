import { z } from 'zod';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const uuidSchema = z.uuid();
const timestampSchema = z.iso.datetime({ offset: true });
const hrefSchema = z.string().startsWith('/g/');
const nonnegativeInteger = z.number().int().nonnegative();

export const responseMetaSchema = z
	.object({
		requestId: uuidSchema
	})
	.strict();

export const apiErrorDataSchema = z
	.object({
		code: z.string().min(1),
		message: z.string().min(1),
		requestId: uuidSchema,
		details: z.unknown().optional()
	})
	.strict();

export const apiErrorEnvelopeSchema = z
	.object({
		error: apiErrorDataSchema
	})
	.strict();

const riotLaneSchema = z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT', 'FLEX']);

export const riotAccountSchema = z
	.object({
		puuid: z.string().min(1),
		platform: z.string().min(1).max(64),
		routingRegion: z.string().min(1).max(64),
		gameName: z.string().min(1),
		tagLine: z.string().min(1),
		normalizedGameName: z.string().min(1),
		normalizedTagLine: z.string().min(1),
		profileIconId: z.number().int().nonnegative().nullable(),
		isMain: z.boolean(),
		mainPosition: riotLaneSchema.nullable(),
		mainPositionUpdatedAt: timestampSchema.nullable(),
		lastSyncedAt: timestampSchema.nullable(),
		createdAt: timestampSchema,
		updatedAt: timestampSchema
	})
	.strict();

export const riotAccountsDataSchema = z.object({
	riotAccounts: z.array(riotAccountSchema)
});

export const riotAccountEnvelopeSchema = z.object({
	riotAccount: riotAccountSchema
});

export const meDataSchema = z
	.object({
		discordId: snowflakeSchema,
		displayName: z.string().min(1),
		avatarUrl: z.url().nullable(),
		defaultGuildId: snowflakeSchema.nullable(),
		serviceAdmin: z.boolean()
	})
	.strict();

export const guildRoleSchema = z.enum(['MEMBER', 'OPERATOR']);

export const guildAccessSummarySchema = z
	.object({
		id: snowflakeSchema,
		displayName: z.string().min(1),
		iconUrl: z.url().nullable(),
		memberCount: nonnegativeInteger,
		currentUserRole: guildRoleSchema
	})
	.strict();

export const meGuildsDataSchema = z
	.object({
		guilds: z.array(guildAccessSummarySchema)
	})
	.strict();

const currentSeasonSchema = z
	.object({
		id: uuidSchema,
		name: z.string().min(1),
		startedAt: timestampSchema
	})
	.strict();

export const guildContextDataSchema = guildAccessSummarySchema
	.extend({
		isExternallyPublic: z.boolean(),
		currentSeason: currentSeasonSchema.nullable()
	})
	.strict();

export const operationSchema = z
	.object({
		id: z.string().min(1),
		kind: z.enum(['ENTRY', 'PICK_BAN', 'AUCTION']),
		title: z.string(),
		status: z.string(),
		detail: z.string(),
		updatedAt: timestampSchema,
		href: hrefSchema,
		priority: z.enum(['urgent', 'normal'])
	})
	.strict();

export const operationsDataSchema = z
	.object({
		operations: z.array(operationSchema).max(100)
	})
	.strict();

export const recruitmentSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string(),
		status: z.literal('CLOSED'),
		targetCount: z.literal(10),
		joinedCount: nonnegativeInteger,
		assignedCount: nonnegativeInteger,
		revision: nonnegativeInteger,
		updatedAt: timestampSchema,
		href: hrefSchema
	})
	.strict();

export const recruitmentListDataSchema = z
	.object({
		recruitments: z.array(recruitmentSchema).max(100),
		nextCursor: z.string().min(1).nullable()
	})
	.strict();

export const seriesSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string(),
		bestOf: z.union([z.literal(1), z.literal(3), z.literal(5)]),
		status: z.enum(['IN_PROGRESS', 'COMPLETED']),
		team1Score: nonnegativeInteger,
		team2Score: nonnegativeInteger,
		winningTeam: z.enum(['TEAM_1', 'TEAM_2']).nullable(),
		terminalReason: z.enum(['SCORE_REACHED', 'EARLY_WIN', 'EARLY_DRAW']).nullable(),
		currentSetNumber: z.number().int().min(1).max(5).nullable(),
		outcome: z.enum(['TEAM_1', 'TEAM_2', 'DRAW']).nullable(),
		updatedAt: timestampSchema,
		endedAt: timestampSchema.nullable(),
		href: hrefSchema
	})
	.strict();

export const seriesListDataSchema = z
	.object({
		series: z.array(seriesSchema).max(100),
		nextCursor: z.string().min(1).nullable()
	})
	.strict();

export const auctionFormatSchema = z.enum(['TEN_PERSON', 'TWENTY_PERSON']);

export const auctionRecruitmentSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string(),
		format: auctionFormatSchema,
		participantCount: z.union([z.literal(10), z.literal(20)]),
		joinedParticipantCount: nonnegativeInteger,
		teamCount: z.union([z.literal(2), z.literal(4)]),
		status: z.literal('CLOSED'),
		revision: nonnegativeInteger,
		updatedAt: timestampSchema,
		href: hrefSchema
	})
	.strict();

export const auctionRecruitmentListDataSchema = z
	.object({
		recruitments: z.array(auctionRecruitmentSchema).max(100),
		nextCursor: z.string().min(1).nullable()
	})
	.strict();

export const auctionMatchSummarySchema = z
	.object({
		id: uuidSchema,
		label: z.enum(['준결승 A', '준결승 B', '결승']),
		matchKey: z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']),
		status: z.enum(['LINEUP_PENDING', 'READY', 'IN_PROGRESS']),
		confirmedGameCount: nonnegativeInteger
	})
	.strict();

const auctionChampionTeamSchema = z
	.object({
		id: uuidSchema,
		name: z.string()
	})
	.strict();

export const auctionTournamentSchema = z
	.object({
		id: uuidSchema,
		displayNumber: z.number().int().positive(),
		title: z.string(),
		format: auctionFormatSchema,
		participantCount: z.union([z.literal(10), z.literal(20)]),
		teamCount: z.union([z.literal(2), z.literal(4)]),
		status: z.enum(['BIDDING', 'BRACKET_SETUP', 'IN_GAME', 'COMPLETED']),
		revision: nonnegativeInteger,
		activeMemberCount: nonnegativeInteger,
		bidMemberCount: nonnegativeInteger,
		incompleteMatchCount: nonnegativeInteger,
		nextMatch: auctionMatchSummarySchema.nullable(),
		championTeam: auctionChampionTeamSchema.nullable(),
		updatedAt: timestampSchema,
		endedAt: timestampSchema.nullable(),
		href: hrefSchema
	})
	.strict();

export const auctionTournamentListDataSchema = z
	.object({
		tournaments: z.array(auctionTournamentSchema).max(100),
		nextCursor: z.string().min(1).nullable()
	})
	.strict();

export type MeData = z.infer<typeof meDataSchema>;
export type GuildAccessSummary = z.infer<typeof guildAccessSummarySchema>;
export type GuildContextData = z.infer<typeof guildContextDataSchema>;
export type OperationDto = z.infer<typeof operationSchema>;
export type RecruitmentDto = z.infer<typeof recruitmentSchema>;
export type SeriesDto = z.infer<typeof seriesSchema>;
export type AuctionRecruitmentDto = z.infer<typeof auctionRecruitmentSchema>;
export type AuctionTournamentDto = z.infer<typeof auctionTournamentSchema>;
export type RiotAccount = z.infer<typeof riotAccountSchema>;
