import { z } from 'zod';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const dummyUserIdSchema = z
	.string()
	.regex(/^dmy_[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
export const userIdSchema = z.union([snowflakeSchema, dummyUserIdSchema]);
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

export const championPreferenceLaneSchema = z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);

export const championPreferenceIdsSchema = z
	.array(
		z
			.string()
			.min(1)
			.max(64)
			.regex(/^[A-Za-z0-9]+$/)
	)
	.max(10)
	.refine((ids) => new Set(ids).size === ids.length, 'Champion preferences must be unique');

export const championPreferencesSchema = z
	.object({
		TOP: championPreferenceIdsSchema,
		JUNGLE: championPreferenceIdsSchema,
		MID: championPreferenceIdsSchema,
		BOTTOM: championPreferenceIdsSchema,
		SUPPORT: championPreferenceIdsSchema
	})
	.strict();

export const championPreferencesDataSchema = z
	.object({
		preferences: championPreferencesSchema
	})
	.strict();

const guildRecordsProfileMemberSchema = z
	.object({
		displayName: z.string().min(1),
		avatarUrl: z.url().nullable(),
		mainLane: championPreferenceLaneSchema.nullable()
	})
	.strict();

const guildRecordsRiotAccountSchema = z
	.object({
		platform: z.literal('KR'),
		gameName: z.string().min(1),
		tagLine: z.string().min(1),
		profileIconId: nonnegativeInteger.nullable(),
		isMain: z.boolean()
	})
	.strict();

export const guildRecordsProfileDataSchema = z
	.object({
		member: guildRecordsProfileMemberSchema,
		championPreferences: championPreferencesSchema,
		riotAccounts: z.array(guildRecordsRiotAccountSchema)
	})
	.strict();

export const guildRecordsSeasonSchema = z
	.object({
		id: uuidSchema,
		name: z.string().min(1).max(100),
		status: z.enum(['ACTIVE', 'CLOSED']),
		startedAt: timestampSchema,
		endedAt: timestampSchema.nullable()
	})
	.strict();

export const guildRecordsSummarySchema = z
	.object({
		games: nonnegativeInteger,
		wins: nonnegativeInteger,
		losses: nonnegativeInteger
	})
	.strict();

export const guildRecordsLaneMmrSchema = z
	.object({
		lane: championPreferenceLaneSchema,
		mmr: z.number().finite(),
		games: nonnegativeInteger,
		wins: nonnegativeInteger
	})
	.strict();

export const guildRecordsChampionRecordSchema = z
	.object({
		championId: z.string().min(1).max(64),
		lane: championPreferenceLaneSchema,
		games: z.number().int().positive(),
		wins: nonnegativeInteger,
		losses: nonnegativeInteger
	})
	.strict();

const guildRecordsChampionIdSchema = z.string().min(1).max(64).nullable();

export const guildRecordsLineupPlayerSchema = z
	.object({
		displayName: z.string().min(1),
		lane: championPreferenceLaneSchema,
		championId: guildRecordsChampionIdSchema,
		isTarget: z.boolean()
	})
	.strict();

export const guildRecordsGameSchema = z
	.object({
		id: uuidSchema,
		gameNumber: z.number().int().min(1).max(5),
		playedAt: timestampSchema,
		winningSide: z.enum(['BLUE', 'RED']),
		target: z
			.object({
				side: z.enum(['BLUE', 'RED']),
				lane: championPreferenceLaneSchema,
				championId: guildRecordsChampionIdSchema,
				mmrBefore: z.number().finite().nullable(),
				mmrAfter: z.number().finite().nullable()
			})
			.strict(),
		blueTeam: z.array(guildRecordsLineupPlayerSchema).length(5),
		redTeam: z.array(guildRecordsLineupPlayerSchema).length(5),
		blueBans: z.array(z.string().min(1).max(64)).max(5),
		redBans: z.array(z.string().min(1).max(64)).max(5)
	})
	.strict()
	.superRefine((game, context) => {
		if ((game.target.mmrBefore === null) !== (game.target.mmrAfter === null)) {
			context.addIssue({
				code: 'custom',
				path: ['target'],
				message: 'Target MMR before and after must both be present or both be null'
			});
		}
	});

const guildRecordsResultSchema = z.enum(['WIN', 'LOSS', 'DRAW']);

export const guildRecordsSeriesRecordSchema = z
	.object({
		kind: z.literal('SERIES'),
		id: uuidSchema,
		source: z.enum(['REGULAR', 'AUCTION']),
		title: z.string().min(1),
		participantCount: z.literal(10),
		playedAt: timestampSchema,
		result: guildRecordsResultSchema,
		teamName: z.string().min(1).nullable(),
		opponentName: z.string().min(1).nullable(),
		stage: z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']).nullable(),
		wins: nonnegativeInteger,
		losses: nonnegativeInteger,
		mmrDelta: z.number().finite().nullable(),
		games: z.array(guildRecordsGameSchema).min(1).max(5)
	})
	.strict();

export const guildRecordsTournamentSeriesSchema = z
	.object({
		id: uuidSchema,
		title: z.string().min(1),
		stage: z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']),
		opponentName: z.string().min(1).nullable(),
		playedAt: timestampSchema,
		result: guildRecordsResultSchema,
		wins: nonnegativeInteger,
		losses: nonnegativeInteger,
		mmrDelta: z.number().finite().nullable(),
		games: z.array(guildRecordsGameSchema).min(1).max(5)
	})
	.strict();

export const guildRecordsTournamentRecordSchema = z
	.object({
		kind: z.literal('TOURNAMENT'),
		id: uuidSchema,
		title: z.string().min(1),
		participantCount: z.literal(20),
		playedAt: timestampSchema,
		teamName: z.string().min(1).nullable(),
		spentPoints: nonnegativeInteger.nullable(),
		placement: z.union([z.literal(1), z.literal(2), z.literal(3)]).nullable(),
		wins: nonnegativeInteger,
		losses: nonnegativeInteger,
		mmrDelta: z.number().finite().nullable(),
		roster: z.array(guildRecordsLineupPlayerSchema).length(5),
		series: z.array(guildRecordsTournamentSeriesSchema).min(1).max(3)
	})
	.strict();

export const guildRecordsRecordSchema = z.discriminatedUnion('kind', [
	guildRecordsSeriesRecordSchema,
	guildRecordsTournamentRecordSchema
]);

const guildRecordsLaneOrder = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;

export const guildRecordsSearchDataSchema = z
	.object({
		member: guildRecordsProfileMemberSchema,
		championPreferences: championPreferencesSchema,
		riotAccounts: z.array(guildRecordsRiotAccountSchema),
		seasons: z.array(guildRecordsSeasonSchema),
		selectedSeasonId: uuidSchema.nullable(),
		summary: guildRecordsSummarySchema,
		laneMmr: z.array(guildRecordsLaneMmrSchema).length(5),
		championRecords: z.array(guildRecordsChampionRecordSchema),
		records: z.array(guildRecordsRecordSchema).max(50),
		nextCursor: z.string().min(1).nullable()
	})
	.strict()
	.superRefine((snapshot, context) => {
		if (snapshot.summary.games !== snapshot.summary.wins + snapshot.summary.losses) {
			context.addIssue({
				code: 'custom',
				path: ['summary'],
				message: 'Summary games must equal wins plus losses'
			});
		}
		for (const [index, entry] of snapshot.laneMmr.entries()) {
			if (entry.lane !== guildRecordsLaneOrder[index]) {
				context.addIssue({
					code: 'custom',
					path: ['laneMmr', index, 'lane'],
					message: 'Lane MMR entries must use canonical lane order'
				});
			}
			if (entry.wins > entry.games) {
				context.addIssue({
					code: 'custom',
					path: ['laneMmr', index, 'wins'],
					message: 'Lane wins cannot exceed games'
				});
			}
		}
		for (const [index, champion] of snapshot.championRecords.entries()) {
			if (champion.games !== champion.wins + champion.losses) {
				context.addIssue({
					code: 'custom',
					path: ['championRecords', index],
					message: 'Champion games must equal wins plus losses'
				});
			}
		}
		for (const [recordIndex, record] of snapshot.records.entries()) {
			if (record.kind === 'SERIES') {
				if (record.games.length !== record.wins + record.losses) {
					context.addIssue({
						code: 'custom',
						path: ['records', recordIndex],
						message: 'Series games must equal wins plus losses'
					});
				}
				validateMmrDelta(record.games, record.mmrDelta, ['records', recordIndex], context);
				continue;
			}
			for (const [seriesIndex, series] of record.series.entries()) {
				if (series.games.length !== series.wins + series.losses) {
					context.addIssue({
						code: 'custom',
						path: ['records', recordIndex, 'series', seriesIndex],
						message: 'Tournament series games must equal wins plus losses'
					});
				}
				validateMmrDelta(
					series.games,
					series.mmrDelta,
					['records', recordIndex, 'series', seriesIndex],
					context
				);
			}
			const tournamentGames = record.series.flatMap((series) => series.games);
			if (record.wins + record.losses !== tournamentGames.length) {
				context.addIssue({
					code: 'custom',
					path: ['records', recordIndex],
					message: 'Tournament games must equal wins plus losses'
				});
			}
			validateMmrDelta(tournamentGames, record.mmrDelta, ['records', recordIndex], context);
		}
		if (
			snapshot.selectedSeasonId !== null &&
			!snapshot.seasons.some((season) => season.id === snapshot.selectedSeasonId)
		) {
			context.addIssue({
				code: 'custom',
				path: ['selectedSeasonId'],
				message: 'Selected season must exist in the season catalog'
			});
		}
	});

export const guildLeaderboardRoleStatSchema = z
	.object({
		mmr: z.number().finite(),
		games: z.number().int().positive(),
		wins: nonnegativeInteger,
		championId: z.string().min(1).max(64).nullable()
	})
	.strict()
	.refine((stat) => stat.wins <= stat.games, 'Lane wins cannot exceed games');

export const guildLeaderboardPlayerSchema = z
	.object({
		id: userIdSchema,
		name: z.string().min(1),
		tag: z.string().min(1).nullable(),
		profileIconId: nonnegativeInteger.nullable(),
		compositeMmr: z.number().finite(),
		mainRole: championPreferenceLaneSchema,
		isMe: z.boolean(),
		stats: z
			.object({
				TOP: guildLeaderboardRoleStatSchema.optional(),
				JUNGLE: guildLeaderboardRoleStatSchema.optional(),
				MID: guildLeaderboardRoleStatSchema.optional(),
				BOTTOM: guildLeaderboardRoleStatSchema.optional(),
				SUPPORT: guildLeaderboardRoleStatSchema.optional()
			})
			.strict()
	})
	.strict()
	.refine((player) => player.stats[player.mainRole] !== undefined, {
		path: ['mainRole'],
		message: 'Main role must refer to a played lane'
	});

export const guildLeaderboardDataSchema = z
	.object({
		season: z
			.object({
				id: uuidSchema,
				name: z.string().min(1).max(100)
			})
			.strict()
			.nullable(),
		players: z.array(guildLeaderboardPlayerSchema)
	})
	.strict()
	.superRefine((snapshot, context) => {
		if (snapshot.season === null && snapshot.players.length > 0) {
			context.addIssue({
				code: 'custom',
				path: ['players'],
				message: 'A leaderboard without an active season cannot contain players'
			});
		}
		const playerIds = snapshot.players.map((player) => player.id);
		if (new Set(playerIds).size !== playerIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['players'],
				message: 'Leaderboard players must be unique'
			});
		}
		if (snapshot.players.filter((player) => player.isMe).length > 1) {
			context.addIssue({
				code: 'custom',
				path: ['players'],
				message: 'Leaderboard can contain at most one current user'
			});
		}
	});

function validateMmrDelta(
	games: Array<z.infer<typeof guildRecordsGameSchema>>,
	actual: number | null,
	path: Array<string | number>,
	context: z.RefinementCtx
) {
	const hasCompleteLedger = games.every(
		(game) => game.target.mmrBefore !== null && game.target.mmrAfter !== null
	);
	const expected = hasCompleteLedger
		? games.reduce(
				(sum, game) => sum + (game.target.mmrAfter as number) - (game.target.mmrBefore as number),
				0
			)
		: null;
	if (
		(actual === null) !== (expected === null) ||
		(actual !== null && expected !== null && Math.abs(actual - expected) > 1e-6)
	) {
		context.addIssue({
			code: 'custom',
			path: [...path, 'mmrDelta'],
			message: 'MMR delta must match the available game ledger values'
		});
	}
}

const riotLaneSchema = z.enum(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT', 'FLEX']);

export const riotAccountSchema = z
	.object({
		puuid: z.string().min(1),
		platform: z.literal('KR'),
		routingRegion: z.literal('ASIA'),
		gameName: z.string().min(1),
		tagLine: z.string().min(1),
		profileIconId: z.number().int().nonnegative().nullable(),
		isMain: z.boolean(),
		mainPosition: riotLaneSchema.nullable(),
		lastSyncedAt: timestampSchema.nullable(),
		createdAt: timestampSchema,
		updatedAt: timestampSchema
	})
	.strict();

export const riotAccountsDataSchema = z
	.object({
		riotAccounts: z.array(riotAccountSchema)
	})
	.strict();

export const privacySettingsSchema = z
	.object({
		publicProfile: z.boolean(),
		showRiotIds: z.boolean(),
		allowExternalRecordSearch: z.boolean()
	})
	.strict();

export const privacySettingsDataSchema = z
	.object({
		settings: privacySettingsSchema
	})
	.strict();

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

export const guildMemberStatusSchema = z.enum(['ACTIVE', 'INACTIVE']);
export const userIdentityKindSchema = z.enum(['DISCORD', 'DUMMY']);

const guildMemberLaneMmrSchema = z
	.object({
		TOP: z.number().finite(),
		JUNGLE: z.number().finite(),
		MID: z.number().finite(),
		BOTTOM: z.number().finite(),
		SUPPORT: z.number().finite()
	})
	.strict();

export const guildAdminMemberSchema = z
	.object({
		id: userIdSchema,
		identityKind: userIdentityKindSchema.default('DISCORD'),
		discordName: z.string().min(1),
		riotId: z.string().min(1).nullable(),
		role: guildRoleSchema,
		mainLane: championPreferenceLaneSchema.nullable(),
		laneMmr: guildMemberLaneMmrSchema,
		registered: z.boolean(),
		status: guildMemberStatusSchema,
		lastActiveAt: timestampSchema.nullable()
	})
	.strict();

export const guildAdminMembersDataSchema = z
	.object({
		revision: nonnegativeInteger,
		updatedAt: timestampSchema,
		updatedBy: userIdSchema.nullable(),
		members: z.array(guildAdminMemberSchema)
	})
	.strict();

export const guildAdminSettingsSchema = z
	.object({
		displayName: z.string().min(1).max(100),
		operatorRoleId: snowflakeSchema,
		recruitmentChannelId: snowflakeSchema.nullable(),
		entryChannelId: snowflakeSchema.nullable(),
		logChannelId: snowflakeSchema.nullable(),
		isExternallyPublic: z.boolean()
	})
	.strict();

const guildAdminRoleOptionSchema = z
	.object({
		id: snowflakeSchema,
		name: z.string().min(1).max(100)
	})
	.strict();

const guildAdminChannelOptionSchema = guildAdminRoleOptionSchema
	.extend({
		parentName: z.string().min(1).max(100).nullable()
	})
	.strict();

export const guildAdminSettingsDataSchema = z
	.object({
		settings: guildAdminSettingsSchema,
		revision: nonnegativeInteger,
		updatedAt: timestampSchema,
		updatedBy: userIdSchema.nullable(),
		catalogUpdatedAt: timestampSchema.nullable(),
		roles: z.array(guildAdminRoleOptionSchema),
		channels: z.array(guildAdminChannelOptionSchema)
	})
	.strict();

export const guildAuditPrincipalTypeSchema = z.enum(['USER', 'BOT', 'SYSTEM', 'MIGRATION']);

export const guildAuditResultSchema = z.enum(['APPLIED', 'REVERTED']);

export const guildAuditEntrySchema = z
	.object({
		id: uuidSchema,
		principalType: guildAuditPrincipalTypeSchema,
		principalId: z.string().min(1),
		principalDisplayName: z.string().min(1).nullable(),
		action: z.string().min(1),
		targetType: z.string().min(1).nullable(),
		targetId: z.string().min(1).nullable(),
		targetDisplayName: z.string().min(1).nullable(),
		checkpointId: uuidSchema.nullable(),
		requestId: z.string().min(1).nullable(),
		beforeSummary: z.json().nullable(),
		afterSummary: z.json().nullable(),
		result: guildAuditResultSchema,
		note: z.string().nullable(),
		createdAt: timestampSchema
	})
	.strict();

export const guildAuditLogsDataSchema = z
	.object({
		entries: z.array(guildAuditEntrySchema).max(100),
		nextCursor: z.string().min(1).nullable()
	})
	.strict();

export const guildAdminSeasonStatusSchema = z.enum(['ACTIVE', 'CLOSED']);

export const guildAdminSeasonSummarySchema = z
	.object({
		id: uuidSchema,
		name: z.string().min(1).max(100),
		status: guildAdminSeasonStatusSchema,
		startedAt: timestampSchema,
		endedAt: timestampSchema.nullable(),
		createdBy: z.string().min(1).nullable(),
		participantCount: nonnegativeInteger,
		generalSeriesCount: nonnegativeInteger,
		auctionTournamentCount: nonnegativeInteger,
		auctionTenCount: nonnegativeInteger,
		auctionTwentyCount: nonnegativeInteger,
		confirmedGameCount: nonnegativeInteger,
		lastGameAt: timestampSchema.nullable()
	})
	.strict();

export const guildAdminSeasonsDataSchema = z
	.object({
		current: guildAdminSeasonSummarySchema,
		completed: z.array(guildAdminSeasonSummarySchema)
	})
	.strict()
	.superRefine((snapshot, context) => {
		if (snapshot.current.status !== 'ACTIVE' || snapshot.current.endedAt !== null) {
			context.addIssue({
				code: 'custom',
				path: ['current'],
				message: 'Current season must be active and open'
			});
		}
		for (const [index, season] of snapshot.completed.entries()) {
			if (season.status !== 'CLOSED' || season.endedAt === null) {
				context.addIssue({
					code: 'custom',
					path: ['completed', index],
					message: 'Completed season must be closed'
				});
			}
		}
	});

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
		status: z.enum(['OPEN', 'CLOSED']),
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
export type ChampionPreferenceLane = z.infer<typeof championPreferenceLaneSchema>;
export type ChampionPreferences = z.infer<typeof championPreferencesSchema>;
export type GuildRecordsProfile = z.infer<typeof guildRecordsProfileDataSchema>;
export type GuildRecordsSeason = z.infer<typeof guildRecordsSeasonSchema>;
export type GuildRecordsLaneMmr = z.infer<typeof guildRecordsLaneMmrSchema>;
export type GuildRecordsChampionRecord = z.infer<typeof guildRecordsChampionRecordSchema>;
export type GuildRecordsLineupPlayer = z.infer<typeof guildRecordsLineupPlayerSchema>;
export type GuildRecordsGame = z.infer<typeof guildRecordsGameSchema>;
export type GuildRecordsSeriesRecord = z.infer<typeof guildRecordsSeriesRecordSchema>;
export type GuildRecordsTournamentSeries = z.infer<typeof guildRecordsTournamentSeriesSchema>;
export type GuildRecordsTournamentRecord = z.infer<typeof guildRecordsTournamentRecordSchema>;
export type GuildRecordsRecord = z.infer<typeof guildRecordsRecordSchema>;
export type GuildRecordsSearchSnapshot = z.infer<typeof guildRecordsSearchDataSchema>;
export type GuildLeaderboardRoleStat = z.infer<typeof guildLeaderboardRoleStatSchema>;
export type GuildLeaderboardPlayer = z.infer<typeof guildLeaderboardPlayerSchema>;
export type GuildLeaderboardSnapshot = z.infer<typeof guildLeaderboardDataSchema>;
export type RiotAccount = z.infer<typeof riotAccountSchema>;
export type PrivacySettings = z.infer<typeof privacySettingsSchema>;
export type GuildMemberStatus = z.infer<typeof guildMemberStatusSchema>;
export type GuildAdminMember = z.infer<typeof guildAdminMemberSchema>;
export type GuildAdminMembersSnapshot = z.infer<typeof guildAdminMembersDataSchema>;
export type GuildAdminSettings = z.infer<typeof guildAdminSettingsSchema>;
export type GuildAdminRoleOption = z.infer<typeof guildAdminRoleOptionSchema>;
export type GuildAdminChannelOption = z.infer<typeof guildAdminChannelOptionSchema>;
export type GuildAdminSettingsSnapshot = z.infer<typeof guildAdminSettingsDataSchema>;
export type GuildAuditPrincipalType = z.infer<typeof guildAuditPrincipalTypeSchema>;
export type GuildAuditResult = z.infer<typeof guildAuditResultSchema>;
export type GuildAuditEntry = z.infer<typeof guildAuditEntrySchema>;
export type GuildAuditLogsSnapshot = z.infer<typeof guildAuditLogsDataSchema>;
export type GuildAdminSeasonStatus = z.infer<typeof guildAdminSeasonStatusSchema>;
export type GuildAdminSeasonSummary = z.infer<typeof guildAdminSeasonSummarySchema>;
export type GuildAdminSeasonsSnapshot = z.infer<typeof guildAdminSeasonsDataSchema>;
