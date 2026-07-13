import { z } from 'zod';

const timestampSchema = z.iso.datetime({ offset: true });
const nonnegativeInteger = z.number().int().nonnegative();
const riotIdComponentSchema = z.string().min(1);
const puuidSchema = z
	.string()
	.min(1)
	.max(256)
	.regex(/^[A-Za-z0-9_-]+$/);

export const riotRecordsLookupSchema = z
	.object({
		gameName: riotIdComponentSchema.max(64),
		tagLine: riotIdComponentSchema.max(16),
		platform: z.literal('KR')
	})
	.strict();

export const riotRecordsAccountSchema = z
	.object({
		puuid: puuidSchema,
		gameName: riotIdComponentSchema.max(64),
		tagLine: riotIdComponentSchema.max(16)
	})
	.strict();

export const riotRankSchema = z
	.object({
		queueType: z.string().min(1),
		tier: z.string().min(1).nullable(),
		rank: z.string().min(1).nullable(),
		leaguePoints: nonnegativeInteger,
		wins: nonnegativeInteger,
		losses: nonnegativeInteger
	})
	.strict();

export const riotChampionAggregateSchema = z
	.object({
		championName: z.string().min(1),
		games: z.number().int().positive(),
		wins: nonnegativeInteger,
		kills: nonnegativeInteger,
		deaths: nonnegativeInteger,
		assists: nonnegativeInteger,
		creepScore: nonnegativeInteger,
		goldEarned: nonnegativeInteger,
		tripleKills: nonnegativeInteger,
		quadraKills: nonnegativeInteger,
		pentaKills: nonnegativeInteger
	})
	.strict();

export const riotMatchParticipantSchema = z
	.object({
		puuid: puuidSchema,
		gameName: riotIdComponentSchema.max(64).nullable(),
		tagLine: riotIdComponentSchema.max(16).nullable(),
		championName: z.string().min(1),
		championLevel: nonnegativeInteger,
		teamId: z.number().int(),
		win: z.boolean(),
		kills: nonnegativeInteger,
		deaths: nonnegativeInteger,
		assists: nonnegativeInteger,
		creepScore: nonnegativeInteger,
		goldEarned: nonnegativeInteger,
		damageDealtToChampions: nonnegativeInteger,
		damageTaken: nonnegativeInteger,
		visionScore: nonnegativeInteger,
		summonerSpellIds: z.tuple([nonnegativeInteger, nonnegativeInteger]),
		itemIds: z.tuple([
			nonnegativeInteger,
			nonnegativeInteger,
			nonnegativeInteger,
			nonnegativeInteger,
			nonnegativeInteger,
			nonnegativeInteger,
			nonnegativeInteger
		]),
		tripleKills: nonnegativeInteger,
		quadraKills: nonnegativeInteger,
		pentaKills: nonnegativeInteger,
		teamPosition: z.string()
	})
	.strict();

export const riotTeamObjectivesSchema = z
	.object({
		baronKills: nonnegativeInteger,
		championKills: nonnegativeInteger,
		dragonKills: nonnegativeInteger,
		hordeKills: nonnegativeInteger,
		inhibitorKills: nonnegativeInteger,
		riftHeraldKills: nonnegativeInteger,
		towerKills: nonnegativeInteger
	})
	.strict();

export const riotMatchTeamSchema = z
	.object({
		teamId: z.number().int(),
		win: z.boolean(),
		objectives: riotTeamObjectivesSchema
	})
	.strict();

export const riotRecentMatchSchema = z
	.object({
		matchId: z.string().min(1).max(96),
		queueId: z.number().int(),
		gameVersion: z.string().min(1),
		durationSeconds: nonnegativeInteger,
		startedAt: timestampSchema,
		targetPuuid: puuidSchema,
		participants: z.array(riotMatchParticipantSchema).max(20),
		teams: z.array(riotMatchTeamSchema).max(2)
	})
	.strict();

export const riotRecordsSampleSchema = z
	.object({
		recentMatches: z
			.object({
				requested: z.number().int().positive(),
				returned: nonnegativeInteger
			})
			.strict(),
		championAggregates: z
			.object({
				seasonStartUnixSeconds: z.number().int().positive(),
				discoveredMatches: nonnegativeInteger,
				analyzedMatches: nonnegativeInteger,
				truncated: z.boolean()
			})
			.strict()
	})
	.strict();

export const riotRecordsSchema = z
	.object({
		account: riotRecordsAccountSchema,
		platform: z.literal('KR'),
		routingRegion: z.literal('ASIA'),
		profileIconId: nonnegativeInteger.nullable(),
		summonerLevel: nonnegativeInteger.nullable(),
		ranks: z.array(riotRankSchema),
		championAggregates: z.array(riotChampionAggregateSchema),
		recentMatches: z.array(riotRecentMatchSchema).max(20),
		sample: riotRecordsSampleSchema,
		refreshedAt: timestampSchema,
		cache: z
			.object({
				state: z.enum(['fresh', 'stale', 'refreshed'])
			})
			.strict()
	})
	.strict();

export const riotRecentMatchesPageSchema = z
	.object({
		account: riotRecordsAccountSchema,
		platform: z.literal('KR'),
		routingRegion: z.literal('ASIA'),
		targetPuuid: puuidSchema,
		matches: z.array(riotRecentMatchSchema).max(20),
		page: z
			.object({
				offset: nonnegativeInteger,
				requested: z.number().int().min(1).max(20),
				returned: nonnegativeInteger,
				hasMore: z.boolean(),
				nextOffset: nonnegativeInteger.nullable()
			})
			.strict()
	})
	.strict();

export type RiotRecordsLookup = z.infer<typeof riotRecordsLookupSchema>;
export type RiotRank = z.infer<typeof riotRankSchema>;
export type RiotChampionAggregate = z.infer<typeof riotChampionAggregateSchema>;
export type RiotMatchParticipant = z.infer<typeof riotMatchParticipantSchema>;
export type RiotRecentMatch = z.infer<typeof riotRecentMatchSchema>;
export type RiotMatchTeam = z.infer<typeof riotMatchTeamSchema>;
export type RiotRecordsSample = z.infer<typeof riotRecordsSampleSchema>;
export type RiotRecentMatchesPage = z.infer<typeof riotRecentMatchesPageSchema>;
export type RiotRecords = z.infer<typeof riotRecordsSchema>;
