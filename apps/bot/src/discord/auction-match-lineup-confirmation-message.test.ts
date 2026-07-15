import { describe, expect, it } from 'vitest';

import {
	auctionMatchLineupConfirmationNotificationPayloadSchema,
	type AuctionMatchLineupConfirmationSnapshot
} from '../api/contracts.js';
import {
	renderAuctionMatchLineupConfirmationMessage,
	renderAuctionMatchLineupConfirmationPng,
	renderAuctionMatchLineupConfirmationSvg
} from './auction-match-lineup-confirmation-message.js';

const guildId = '200000000000000001';
const matchId = '01900000-0000-7000-8000-000000000601';
const webOrigin = new URL('https://dev.monkeybot.lol');

function snapshot(): AuctionMatchLineupConfirmationSnapshot {
	return {
		tournament: {
			id: '01900000-0000-7000-8000-000000000501',
			displayNumber: 42,
			title: '금요 경매 내전',
			format: 'TWENTY_PERSON',
			hardFearless: true
		},
		match: {
			id: matchId,
			matchKey: 'FINAL',
			label: '결승',
			bestOf: 3
		},
		teams: [
			{
				teamId: '01900000-0000-7000-8000-000000000701',
				name: '블루팀',
				players: players('100000000000000001', 'Blue')
			},
			{
				teamId: '01900000-0000-7000-8000-000000000702',
				name: '레드팀',
				players: players('100000000000000006', 'Red')
			}
		],
		links: { pickBan: `/g/${guildId}/auction/matches/${matchId}` }
	};
}

function players(firstId: string, prefix: string) {
	const idPrefix = firstId.slice(0, -2);
	const firstSuffix = Number.parseInt(firstId.slice(-2), 10);
	const idAt = (index: number) => `${idPrefix}${String(firstSuffix + index).padStart(2, '0')}`;
	return [
		{ id: idAt(0), displayName: `${prefix} Top`, lane: 'TOP' as const },
		{ id: idAt(1), displayName: `${prefix} Jungle`, lane: 'JUNGLE' as const },
		{ id: idAt(2), displayName: `${prefix} Mid`, lane: 'MID' as const },
		{ id: idAt(3), displayName: `${prefix} Bottom`, lane: 'BOTTOM' as const },
		{ id: idAt(4), displayName: `${prefix} Support`, lane: 'SUPPORT' as const }
	];
}

describe('renderAuctionMatchLineupConfirmationMessage', () => {
	it('renders an entry-style auction lineup PNG card and match workspace link', () => {
		const current = snapshot();
		const rendered = renderAuctionMatchLineupConfirmationMessage(current, webOrigin);
		const svg = renderAuctionMatchLineupConfirmationSvg(current);
		const png = renderAuctionMatchLineupConfirmationPng(current);

		expect(svg).toContain('20인 경매 내전 · 결승 라인업 확정');
		expect(svg).toContain('블루팀');
		expect(svg).toContain('레드팀');
		expect(png.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
		expect(rendered.files[0]).toMatchObject({ name: 'auction-42-final-lineup.png' });
		expect(JSON.stringify(rendered.components)).toContain(
			`https://dev.monkeybot.lol/g/${guildId}/auction/matches/${matchId}`
		);
	});

	it('escapes player text and rejects a payload whose match or teams do not agree', () => {
		const current = snapshot();
		current.teams[0]!.players[0]!.displayName = '<Longcat & Co>';

		const svg = renderAuctionMatchLineupConfirmationSvg(current);
		expect(svg).toContain('&lt;Longcat &amp; Co&gt;');
		expect(svg).not.toContain('<Longcat & Co>');

		const payload = { schemaVersion: 1 as const, matchId, snapshot: current };
		expect(auctionMatchLineupConfirmationNotificationPayloadSchema.safeParse(payload).success).toBe(
			true
		);
		expect(
			auctionMatchLineupConfirmationNotificationPayloadSchema.safeParse({
				...payload,
				matchId: '01900000-0000-7000-8000-000000000602'
			}).success
		).toBe(false);
		expect(
			auctionMatchLineupConfirmationNotificationPayloadSchema.safeParse({
				...payload,
				snapshot: {
					...current,
					teams: [current.teams[0], { ...current.teams[1]!, teamId: current.teams[0]!.teamId }]
				}
			}).success
		).toBe(false);
	});
});
