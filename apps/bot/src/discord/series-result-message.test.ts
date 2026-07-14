import { describe, expect, it } from 'vitest';

import type {
	SeriesResultNotificationV1Snapshot,
	SeriesResultNotificationV2Snapshot
} from '../api/contracts.js';
import {
	renderSeriesResultMessage,
	renderSeriesResultPng,
	renderSeriesResultSvg,
	type SeriesResultImageResolver
} from './series-result-message.js';

const guildId = '200000000000000001';
const seriesId = '01900000-0000-7000-8000-000000000401';
const webOrigin = new URL('https://dev.monkeybot.lol');
const iconPng = Buffer.from(
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL5igAAAABJRU5ErkJggg==',
	'base64'
);
const testImageResolver: SeriesResultImageResolver = async (resvg) => {
	for (const href of new Set(resvg.imagesToResolve())) {
		resvg.resolveImage(href, iconPng);
	}
};

describe('renderSeriesResultMessage', () => {
	it('keeps rendering an escaped compact schema v1 card and a result deep link', async () => {
		const current = snapshot();
		current.series.title = '<Longcat & Co.> 내전';

		const svg = renderSeriesResultSvg(current);
		const png = await renderSeriesResultPng(current, { resolveImages: testImageResolver });
		const message = JSON.parse(
			JSON.stringify(
				await renderSeriesResultMessage(current, webOrigin, { resolveImages: testImageResolver })
			)
		) as {
			components: unknown;
		};

		expect(svg).toContain('&lt;Longcat &amp; Co.&gt; 내전');
		expect(svg).not.toContain('<Longcat & Co.> 내전');
		expect(svg).toContain('정규 종료');
		expect(svg).toContain('1팀 승리');
		expect(svg).toContain('font-size="48" font-weight="700" fill="#f3f6fb">2</text>');
		expect(svg).toContain('font-size="48" font-weight="700" fill="#f3f6fb">0</text>');
		expect(svg).toContain('1세트');
		expect(svg).toContain('2세트');
		expect(png.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
		expect(JSON.stringify(message.components)).toContain(
			`https://dev.monkeybot.lol/g/${guildId}/series/${seriesId}/result`
		);
	});

	it('renders the compact v2 card with one roster and champion image tiles for every set', async () => {
		const current = fullSnapshot();
		current.teams[0]!.name = '<Blue & Team>';
		current.teams[0]!.players[0]!.displayName = '<Longcat & Co.>';
		current.games[0]!.team1.picks[0]!.displayName = '<Longcat & Co.>';
		current.games[0]!.team1.picks[1]!.championId = null;
		current.games[0]!.bans.blue[0] = null;
		current.games[0]!.bans.red[4] = 'UnreasonablyLongChampionIdentifier';
		current.games[0]!.patchVersion = null;
		current.games[1]!.team1.picks[0]!.championId = 'Bad/Champion';

		const svg = renderSeriesResultSvg(current);
		const png = await renderSeriesResultPng(current, { resolveImages: testImageResolver });

		expect(svg).toContain('&lt;Blue &amp; Team&gt;');
		expect(svg).toContain('&lt;Longcat &amp; Co.&gt;');
		expect(svg).not.toContain('<Blue & Team>');
		expect(svg).not.toContain('<Longcat & Co.>');
		expect(svg).toContain('참가자');
		expect(svg.match(/&lt;Longcat &amp; Co\.&gt;/g) ?? []).toHaveLength(1);
		expect(svg).toContain('세트 기록');
		expect(svg).not.toContain('BLUE BAN');
		expect(svg).not.toContain('RED BAN');
		expect(svg).not.toContain('패치 미입력');
		expect(svg).toContain(
			'href="https://ddragon.leagueoflegends.com/cdn/15.14.1/img/champion/LeeSin.png"'
		);
		expect(svg.match(/<image href=/g)).toHaveLength(19);
		expect(svg).not.toContain('>LeeSin</text>');
		expect(svg).not.toContain('Bad/Champion');
		expect(svg).not.toContain('UnreasonablyLongChampionIdentifier');
		expect(svg).toContain('height="978"');
		expect(png.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
	});

	it('labels an operator-confirmed early win separately from a score-reached result', () => {
		const current = snapshot({
			series: { terminalReason: 'EARLY_WIN' },
			score: { team1: 1, team2: 0 },
			games: [{ setNumber: 1, winner: 'TEAM_1' }]
		});

		const svg = renderSeriesResultSvg(current);

		expect(svg).toContain('조기 종료 · 승리 처리');
		expect(svg).toContain('1팀 승리');
	});

	it('renders an early draw and grows to show every confirmed BO5 set', () => {
		const current = snapshot({
			series: {
				winningTeam: null,
				terminalReason: 'EARLY_DRAW',
				bestOf: 5
			},
			score: { team1: 2, team2: 2 },
			games: [
				{ setNumber: 1, winner: 'TEAM_1' },
				{ setNumber: 2, winner: 'TEAM_2' },
				{ setNumber: 3, winner: 'TEAM_1' },
				{ setNumber: 4, winner: 'TEAM_2' }
			]
		});

		const svg = renderSeriesResultSvg(current);

		expect(svg).toContain('조기 종료 · 무승부');
		expect(svg).toContain('무승부');
		expect(svg).toContain('4세트');
		expect(svg).toContain('height="648"');
	});
});

function snapshot(
	overrides: {
		series?: Partial<SeriesResultNotificationV1Snapshot['series']>;
		score?: Partial<SeriesResultNotificationV1Snapshot['score']>;
		games?: SeriesResultNotificationV1Snapshot['games'];
	} = {}
): SeriesResultNotificationV1Snapshot {
	return {
		series: {
			id: seriesId,
			displayNumber: 12,
			title: '일반 내전 #12',
			bestOf: 3,
			hardFearless: true,
			winningTeam: 'TEAM_1',
			terminalReason: 'SCORE_REACHED',
			...overrides.series
		},
		score: { team1: 2, team2: 0, ...overrides.score },
		games: overrides.games ?? [
			{ setNumber: 1, winner: 'TEAM_1' },
			{ setNumber: 2, winner: 'TEAM_1' }
		],
		links: { result: `/g/${guildId}/series/${seriesId}/result` }
	};
}

function fullSnapshot(): SeriesResultNotificationV2Snapshot {
	return {
		series: {
			id: seriesId,
			displayNumber: 12,
			title: '일반 내전 #12',
			bestOf: 3,
			hardFearless: true,
			winningTeam: 'TEAM_1',
			terminalReason: 'SCORE_REACHED'
		},
		teams: [
			{
				team: 'TEAM_1',
				name: '1팀',
				players: [
					{ id: '100000000000000001', displayName: 'Longcat', lane: 'TOP' },
					{ id: '100000000000000002', displayName: 'Jungle', lane: 'JUNGLE' },
					{ id: '100000000000000003', displayName: 'Mid', lane: 'MID' },
					{ id: '100000000000000004', displayName: 'Bottom', lane: 'BOTTOM' },
					{ id: '100000000000000005', displayName: 'Support', lane: 'SUPPORT' }
				]
			},
			{
				team: 'TEAM_2',
				name: '2팀',
				players: [
					{ id: '100000000000000006', displayName: 'Top Two', lane: 'TOP' },
					{ id: '100000000000000007', displayName: 'Jungle Two', lane: 'JUNGLE' },
					{ id: '100000000000000008', displayName: 'Mid Two', lane: 'MID' },
					{ id: '100000000000000009', displayName: 'Bottom Two', lane: 'BOTTOM' },
					{ id: '100000000000000010', displayName: 'Support Two', lane: 'SUPPORT' }
				]
			}
		],
		score: { team1: 2, team2: 0 },
		games: [fullGame(1, 'TEAM_1', 'DEFAULT'), fullGame(2, 'TEAM_1', 'SWAPPED')],
		links: { result: `/g/${guildId}/series/${seriesId}/result` }
	};
}

function fullGame(
	setNumber: number,
	winner: 'TEAM_1' | 'TEAM_2',
	sidePreset: 'DEFAULT' | 'SWAPPED'
): SeriesResultNotificationV2Snapshot['games'][number] {
	const team1 = fullSnapshotTeam('TEAM_1');
	const team2 = fullSnapshotTeam('TEAM_2');
	return {
		setNumber,
		winner,
		sidePreset,
		team1: {
			team: team1.team,
			name: team1.name,
			picks: team1.players.map((player, index) => ({
				...player,
				championId: ['Garen', 'LeeSin', 'Ahri', 'Jinx', 'Lulu'][index]!
			}))
		},
		team2: {
			team: team2.team,
			name: team2.name,
			picks: team2.players.map((player, index) => ({
				...player,
				championId: ['Darius', 'Viego', 'Orianna', 'KaiSa', 'Nautilus'][index]!
			}))
		},
		bans: {
			blue: ['Yone', 'Rumble', 'Aatrox', 'Syndra', 'Draven'],
			red: ['Aurora', 'Sejuani', 'Azir', 'Aphelios', 'Rakan']
		},
		patchVersion: '15.14'
	};
}

function fullSnapshotTeam(
	team: 'TEAM_1' | 'TEAM_2'
): SeriesResultNotificationV2Snapshot['teams'][number] {
	return fullSnapshotTeams().find((candidate) => candidate.team === team)!;
}

function fullSnapshotTeams(): SeriesResultNotificationV2Snapshot['teams'] {
	return [
		{
			team: 'TEAM_1',
			name: '1팀',
			players: [
				{ id: '100000000000000001', displayName: 'Longcat', lane: 'TOP' },
				{ id: '100000000000000002', displayName: 'Jungle', lane: 'JUNGLE' },
				{ id: '100000000000000003', displayName: 'Mid', lane: 'MID' },
				{ id: '100000000000000004', displayName: 'Bottom', lane: 'BOTTOM' },
				{ id: '100000000000000005', displayName: 'Support', lane: 'SUPPORT' }
			]
		},
		{
			team: 'TEAM_2',
			name: '2팀',
			players: [
				{ id: '100000000000000006', displayName: 'Top Two', lane: 'TOP' },
				{ id: '100000000000000007', displayName: 'Jungle Two', lane: 'JUNGLE' },
				{ id: '100000000000000008', displayName: 'Mid Two', lane: 'MID' },
				{ id: '100000000000000009', displayName: 'Bottom Two', lane: 'BOTTOM' },
				{ id: '100000000000000010', displayName: 'Support Two', lane: 'SUPPORT' }
			]
		}
	];
}
