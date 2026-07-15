import { describe, expect, it } from 'vitest';
import {
	auctionTournamentResultDeleteNotificationPayloadSchema,
	auctionTournamentResultNotificationPayloadSchema,
	type AuctionTournamentResultDeleteNotificationPayload,
	type AuctionTournamentResultNotificationPayload
} from '../api/contracts.js';
import { renderAuctionTournamentResultMessage } from './auction-tournament-result-message.js';

const guildId = '200000000000000001';
const channelId = '500000000000000001';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const webOrigin = new URL('https://dev.monkeybot.lol');

function payload(): AuctionTournamentResultNotificationPayload {
	return {
		schemaVersion: 1,
		guildId,
		tournamentId,
		channelId,
		tournament: {
			displayNumber: 42,
			title: '금요 경매 내전',
			format: 'TWENTY_PERSON',
			semifinalBestOf: 1,
			finalBestOf: 3,
			hardFearless: true
		},
		champion: {
			teamId: '01900000-0000-7000-8000-000000000711',
			teamName: '블루팀'
		},
		matches: [
			{
				matchKey: 'SEMIFINAL_A',
				label: '준결승 A',
				bestOf: 1,
				leftTeamName: '블루팀',
				rightTeamName: '레드팀',
				leftScore: 1,
				rightScore: 0,
				winnerTeamName: '블루팀'
			},
			{
				matchKey: 'SEMIFINAL_B',
				label: '준결승 B',
				bestOf: 1,
				leftTeamName: '그린팀',
				rightTeamName: '퍼플팀',
				leftScore: 0,
				rightScore: 1,
				winnerTeamName: '퍼플팀'
			},
			{
				matchKey: 'FINAL',
				label: '결승',
				bestOf: 3,
				leftTeamName: '블루팀',
				rightTeamName: '퍼플팀',
				leftScore: 2,
				rightScore: 1,
				winnerTeamName: '블루팀'
			}
		],
		completedAt: '2026-07-14T12:00:00Z'
	};
}

function deletePayload(): AuctionTournamentResultDeleteNotificationPayload {
	return {
		schemaVersion: 1,
		guildId,
		tournamentId,
		channelId,
		discordMessageId: '600000000000000001'
	};
}

function json(value: unknown): Record<string, unknown> {
	return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
}

describe('renderAuctionTournamentResultMessage', () => {
	it('renders a compact embed with the champion, rule snapshot, completed bracket, and result link', () => {
		const rendered = json(renderAuctionTournamentResultMessage(payload(), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('경매 내전 완료 #42');
		expect(serialized).toContain('블루팀');
		expect(serialized).toContain('4강 BO1 · 결승 BO3 · 하드 피어리스');
		expect(serialized).toContain('준결승 A');
		expect(serialized).toContain('결승');
		expect(serialized).toContain('결과 보기');
		expect(rendered.embeds).toHaveLength(1);
		expect(JSON.stringify(rendered.components)).toContain(
			`https://dev.monkeybot.lol/g/${guildId}/auction/tournaments/${tournamentId}/result`
		);
		expect(rendered).not.toHaveProperty('files');
	});

	it('rejects a malformed result that does not contain the completed format bracket', () => {
		const current = payload();
		current.matches = current.matches.filter((match) => match.matchKey !== 'SEMIFINAL_B');

		expect(auctionTournamentResultNotificationPayloadSchema.safeParse(current).success).toBe(false);
	});

	it('uses a required null semifinal rule for a completed ten-person tournament', () => {
		const current = payload();
		current.tournament = {
			...current.tournament,
			format: 'TEN_PERSON',
			semifinalBestOf: null
		};
		current.matches = [current.matches.at(-1)!];

		expect(auctionTournamentResultNotificationPayloadSchema.safeParse(current).success).toBe(true);
		current.tournament.semifinalBestOf = 1;
		expect(auctionTournamentResultNotificationPayloadSchema.safeParse(current).success).toBe(false);
	});

	it('strictly validates the result deletion payload independently from the result snapshot', () => {
		expect(
			auctionTournamentResultDeleteNotificationPayloadSchema.safeParse(deletePayload()).success
		).toBe(true);
		expect(
			auctionTournamentResultDeleteNotificationPayloadSchema.safeParse({
				...deletePayload(),
				unexpected: true
			}).success
		).toBe(false);
		expect(
			auctionTournamentResultDeleteNotificationPayloadSchema.safeParse({
				...deletePayload(),
				discordMessageId: 'not-a-snowflake'
			}).success
		).toBe(false);
	});
});
