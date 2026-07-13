import { describe, expect, it } from 'vitest';
import { riotRecordsSchema, type RiotRecords } from './riotRecords';

const records: RiotRecords = {
	account: { puuid: 'puuid-main', gameName: 'Longcat', tagLine: 'KR1' },
	platform: 'KR',
	routingRegion: 'ASIA',
	profileIconId: 12,
	summonerLevel: 300,
	ranks: [
		{
			queueType: 'RANKED_SOLO_5x5',
			tier: 'PLATINUM',
			rank: 'III',
			leaguePoints: 23,
			wins: 22,
			losses: 29
		}
	],
	championAggregates: [
		{
			championName: 'LeeSin',
			games: 3,
			wins: 2,
			kills: 18,
			deaths: 12,
			assists: 30,
			creepScore: 510,
			goldEarned: 36_000,
			tripleKills: 1,
			quadraKills: 0,
			pentaKills: 0
		}
	],
	recentMatches: [
		{
			matchId: 'KR_123',
			queueId: 420,
			gameVersion: '16.13.1',
			durationSeconds: 1_800,
			startedAt: '2026-07-13T00:00:00Z',
			targetPuuid: 'puuid-main',
			participants: [
				{
					puuid: 'puuid-main',
					gameName: 'Longcat',
					tagLine: 'KR1',
					championName: 'LeeSin',
					championLevel: 16,
					teamId: 100,
					win: true,
					kills: 8,
					deaths: 4,
					assists: 9,
					creepScore: 180,
					goldEarned: 12_000,
					damageDealtToChampions: 20_000,
					damageTaken: 18_000,
					visionScore: 35,
					summonerSpellIds: [4, 11],
					itemIds: [6692, 3047, 3071, 3053, 6333, 3364, 0],
					tripleKills: 1,
					quadraKills: 0,
					pentaKills: 0,
					teamPosition: 'JUNGLE'
				}
			],
			teams: [
				{
					teamId: 100,
					win: true,
					objectives: {
						baronKills: 1,
						championKills: 25,
						dragonKills: 3,
						hordeKills: 0,
						inhibitorKills: 2,
						riftHeraldKills: 1,
						towerKills: 9
					}
				}
			]
		}
	],
	sample: {
		recentMatches: { requested: 10, returned: 1 },
		championAggregates: {
			seasonStartUnixSeconds: 1_767_841_200,
			discoveredMatches: 3,
			analyzedMatches: 3,
			truncated: false
		}
	},
	refreshedAt: '2026-07-13T00:01:00Z',
	cache: { state: 'fresh' }
};

describe('Riot records contract', () => {
	it('accepts the complete provider-neutral records snapshot', () => {
		expect(riotRecordsSchema.parse(records)).toEqual(records);
	});

	it('rejects partial, extended, and incorrectly shaped snapshots', () => {
		expect(riotRecordsSchema.safeParse({ ...records, recentMatches: undefined }).success).toBe(
			false
		);
		expect(riotRecordsSchema.safeParse({ ...records, providerPayload: {} }).success).toBe(false);
		expect(
			riotRecordsSchema.safeParse({
				...records,
				recentMatches: [
					{
						...records.recentMatches[0]!,
						participants: [
							{
								...records.recentMatches[0]!.participants[0],
								itemIds: [6692]
							}
						]
					}
				]
			}).success
		).toBe(false);
	});
});
